#!/usr/bin/env python3
"""Standalone measurement orchestration; strict conditions and output validation."""
import json
import argparse
import hashlib
import math
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path

OPS = ("build", "next_line", "all_lines", "intrinsic", "reuse_widths", "rebuild_widths", "page_retry")
PHASES = ("context_init", "font_initialization_registration", "build", "all_lines")
SCOPES = ("font_context_init", "build", "plain_lines", "release_plain_lines", "justify_lines", "release_justify_lines", "reuse_widths", "release_reuse", "rebuild_widths", "release_rebuild", "page_retry", "release_pages", "intrinsic", "release_intrinsic", "drop_paragraphs", "context_shrink_zero", "drop_context", "drop_fonts")
SOURCE_FINGERPRINT_VERSION = 2
CONDITIONS = ("rustc", "cargo", "cpu", "os", "features", "profile", "flags", "build_configuration", "font_hashes", "input_hash", "harness_hash", "lock_hash", "measurement_config", "source_fingerprint_version")
ROOT = Path(__file__).resolve().parents[2]
VARIANTS = ("many-short-latin", "nested-atomic", "preserved-tabs", "float-retry", "justify", "fallback")

def validate_config(target,case,cold_samples):
    require(type(cold_samples) is int and cold_samples>=2, "cold-samples must be at least two")
    ids={c["id"] for c in json.loads((ROOT/"dev/fixtures/assets/cases.json").read_text())}|set(VARIANTS)
    require(case is None or case in ids, "unknown/empty selected workload")
    if Path(target).exists():raise FileExistsError(target)

def load_timings(root,selected):
    root=Path(root)
    evidence=json.loads((root/"digests.json").read_text())
    digests=keyed([dict(key=f'{row["settings"]["id"]}/{row["settings"]["scale"]}',**row) for row in evidence])
    require(digests.keys()==keyed(selected).keys(), "timing case set differs")
    result={}
    for row in selected:
        settings=row["settings"];key=row["key"]
        require(digests[key]["settings"]==settings, "timing settings differ")
        require(set(digests[key]["operations"])==set(OPS), "missing timing digest")
        times={}
        for op in OPS:
            sample=json.loads((root/settings["id"]/op/str(settings["scale"])/"new/sample.json").read_text())
            iters=sample.get("iters",[]);nanos=sample.get("times",[])
            require(len(iters)==len(nanos) and len(iters)>=10, "missing raw Criterion samples")
            for it,ns in zip(iters,nanos):number(it,positive=True);number(ns,positive=True)
            times[op]=dict(digest=digests[key]["operations"][op],sample=sample,median_ns=statistics.median(ns/it for it,ns in zip(iters,nanos)))
        result[key]=times
    return result

def require(condition, message):
    if not condition:
        raise ValueError(message)

def number(value, *, positive=False, integer=False):
    require(type(value) in ((int,) if integer else (int,float)), "invalid numeric value")
    require(math.isfinite(value) and (value>0 if positive else value>=0), "negative/nonfinite measurement")

def validate_digest(d):
    require(isinstance(d,dict), "missing digest")
    sha=d.get("sha256")
    require(isinstance(sha,str) and len(sha)==64 and all(c in "0123456789abcdef" for c in sha), "invalid digest hash")
    for field in ("lines","glyphs","runs","synthetic_glyphs","float_reports","height_retries","intrinsic_measurements"):
        number(d.get(field),integer=True)
    require(d["synthetic_glyphs"]==0, "synthetic output cannot be measured as fixed-font output")

def keyed(values):
    require(isinstance(values,list) and values, "empty measurement set")
    result={}
    for row in values:
        require(isinstance(row,dict) and isinstance(row.get("key"),str), "invalid case key")
        require(row["key"] not in result, "duplicate measurement")
        require(isinstance(row.get("settings"),dict), "missing input/settings")
        require(row["key"]==f'{row["settings"].get("id")}/{row["settings"].get("scale")}', "case key differs from settings")
        result[row["key"]]=row
    return result

def validate_report(report):
    require(isinstance(report,dict) and report.get("schema")==1, "unsupported report schema")
    metadata=report.get("metadata",{})
    require(isinstance(metadata.get("revision"),str) and metadata["revision"], "missing revision")
    conditions=metadata.get("conditions",{})
    require(isinstance(conditions,dict) and all(k in conditions for k in CONDITIONS), "missing conditions")
    version=conditions["source_fingerprint_version"]
    require(type(version) is int and version==SOURCE_FINGERPRINT_VERSION, "unsupported source fingerprint coverage")
    source=metadata.get("source_hash")
    require(isinstance(source,str) and len(source)==64 and all(c in "0123456789abcdef" for c in source), "missing/invalid engine source hash")
    selected=keyed(report.get("selected"));rows=keyed(report.get("rows"))
    require(rows.keys()==selected.keys(), "missing or extra measured cases")
    for key,row in rows.items():
        settings=selected[key]["settings"]
        number(settings.get("paragraphs"),positive=True,integer=True)
        require(row["settings"]==settings, "settings changed while measuring")
        timing=row.get("timing",{})
        require(set(timing)==set(OPS), "missing timing operation")
        for operation,t in timing.items():
            validate_digest(t.get("digest"));number(t.get("median_ns"),positive=True)
            sample=t.get("sample",{});iters=sample.get("iters",[]);times=sample.get("times",[])
            require(isinstance(iters,list) and isinstance(times,list) and len(iters)==len(times) and len(times)>=10, "invalid raw timing sample")
            for it,ns in zip(iters,times):number(it,positive=True);number(ns,positive=True)
            require(math.isclose(t["median_ns"],statistics.median(ns/it for it,ns in zip(iters,times)),rel_tol=1e-12), "summary differs from raw sample")
            d=t["digest"]
            if operation=="intrinsic":
                require(d["intrinsic_measurements"]==settings["paragraphs"] and d["lines"]==d["glyphs"]==d["runs"]==0, "intrinsic output incomplete")
            else:
                require(d["lines"]>=settings["paragraphs"] and d["glyphs"]>0 and d["runs"]>0 and d["intrinsic_measurements"]==0, "layout output incomplete")
                if operation=="next_line":require(d["lines"]==settings["paragraphs"], "first line output incomplete")
        cold=row.get("cold");wall=row.get("process_wall_ns")
        require(isinstance(cold,list) and len(cold)>=2 and isinstance(wall,list) and len(wall)==len(cold), "missing cold samples")
        for child,ns in zip(cold,wall):
            number(ns,positive=True,integer=True)
            require(child.get("schema")==1 and child.get("mode")=="cold" and child.get("instrumented") is False, "cold sample uses wrong build")
            require(child.get("settings")==settings, "cold input differs")
            phases=child.get("durations_ns",{})
            require(set(phases)==set(PHASES), "missing cold phases")
            for value in phases.values():number(value,integer=True)
            require(ns>=sum(phases.values()), "process wall shorter than measured engine phases")
            validate_digest(child.get("digest"))
            require(child["digest"]==timing["all_lines"]["digest"], "cold and warm outputs differ")
        memory=row.get("memory",{})
        require(memory.get("schema")==1 and memory.get("mode")=="memory" and memory.get("instrumented") is True and "durations_ns" not in memory, "invalid memory build")
        require(memory.get("settings")==settings, "memory input differs")
        scopes=memory.get("scopes",[])
        require(isinstance(scopes,list) and len(scopes)==len(SCOPES) and {x.get("name") for x in scopes}==set(SCOPES), "missing/duplicate memory scopes")
        for scope in scopes:
            c=scope.get("counts",{})
            for field in ("calls","allocated_bytes","deallocated_bytes","start_live_bytes","live_bytes","peak_extra_bytes"):number(c.get(field),integer=True)
            net=c.get("net_bytes");require(type(net) is int, "invalid signed net")
            require(c["allocated_bytes"]-c["deallocated_bytes"]==net and c["live_bytes"]-c["start_live_bytes"]==net, "memory counters inconsistent")
            require(max(net,0)<=c["peak_extra_bytes"]<=c["allocated_bytes"], "memory peak inconsistent")
        d=memory.get("digests",{})
        require(set(d)=={"default","plain","justify","reuse","rebuild","pages","intrinsic"}, "missing memory digests")
        for value in d.values():validate_digest(value)
        require(d["default"]==timing["all_lines"]["digest"]==timing["build"]["digest"], "build/layout memory output differs")
        require(d["reuse"]==d["rebuild"]==timing["reuse_widths"]["digest"]==timing["rebuild_widths"]["digest"], "reuse/rebuild outputs differ")
        require(d["pages"]==timing["page_retry"]["digest"] and d["pages"]["height_retries"]==d["pages"]["lines"], "page retry output incomplete")
        require(d["intrinsic"]==timing["intrinsic"]["digest"], "intrinsic output differs")

def compare(current,baseline):
    validate_report(current);validate_report(baseline)
    require(current["metadata"]["conditions"]==baseline["metadata"]["conditions"], "baseline measurement conditions differ")
    new=keyed(current["rows"]);old=keyed(baseline["rows"])
    require(new.keys()==old.keys(), "baseline case set differs")
    rows=[]
    for key,row in new.items():
        previous=old[key]
        require(row["settings"]==previous["settings"], "baseline settings differ")
        for operation in OPS:require(row["timing"][operation]["digest"]==previous["timing"][operation]["digest"], "baseline output digest differs")
        now_memory={s["name"]:s["counts"] for s in row["memory"]["scopes"]};before_memory={s["name"]:s["counts"] for s in previous["memory"]["scopes"]}
        cold_ratios={}
        for phase in PHASES:
            before=statistics.median(s["durations_ns"][phase] for s in previous["cold"])
            after=statistics.median(s["durations_ns"][phase] for s in row["cold"])
            cold_ratios[phase]=after/before if before else None
        rows.append(dict(key=key,timing_ratios={op:row["timing"][op]["median_ns"]/previous["timing"][op]["median_ns"] for op in OPS},cold_median_ratios=cold_ratios,process_wall_median_ratio=statistics.median(row["process_wall_ns"])/statistics.median(previous["process_wall_ns"]),memory_net_deltas={name:now_memory[name]["net_bytes"]-before_memory[name]["net_bytes"] for name in SCOPES},memory_peak_deltas={name:now_memory[name]["peak_extra_bytes"]-before_memory[name]["peak_extra_bytes"] for name in SCOPES}))
    return dict(schema=1,baseline_revision=baseline["metadata"]["revision"],current_revision=current["metadata"]["revision"],rows=rows)

def publish(target,collect):
    target=Path(target)
    if target.exists():raise FileExistsError(target)
    target.parent.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=target.name+"-staging-",dir=target.parent) as temporary:
        stage=Path(temporary);report=collect(stage);validate_report(report)
        (stage/"results.json").write_text(json.dumps(report,indent=2,allow_nan=False)+"\n",encoding="utf-8")
        if target.exists():raise FileExistsError(target)
        stage.rename(target)

def execute(argv,log,*,cwd=ROOT,env=None):
    """Use argv, retain diagnostics, and never accept a failed child as data."""
    result=subprocess.run([str(x) for x in argv],cwd=cwd,env=env,capture_output=True,text=True)
    Path(log).write_text(result.stdout+result.stderr,encoding="utf-8")
    result.check_returncode()
    return result.stdout

def ensure_lock(root,log,env=None):
    if not (Path(root)/"Cargo.lock").is_file():
        execute(["cargo","+stable","generate-lockfile"],log,cwd=root,env=env)

def executable_from_cargo(output,name,kind,*,profiles=None):
    found=[]
    for line in output.splitlines():
        row=json.loads(line)
        if row.get("reason")=="compiler-artifact" and row.get("target",{}).get("name")==name and kind in row["target"].get("kind",[]) and row.get("executable"):
            found.append(row)
    require(len(found)==1, "missing/ambiguous Cargo executable")
    if profiles is not None:
        require(isinstance(found[0].get("profile"),dict), "missing actual Cargo profile")
        profiles[name]=found[0]["profile"]
    return Path(found[0]["executable"])

def file_hash(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def build_configuration(root,env):
    """Conservative compatibility fingerprint; config contents stay private."""
    root=Path(root).resolve()
    configs={}
    locations=[(f"ancestor-{depth}",directory/".cargo") for depth,directory in enumerate([root,*root.parents])]
    home=Path(env.get("CARGO_HOME",str(Path.home()/".cargo")))
    if not home.is_absolute():home=root/home
    locations.append(("cargo-home",home))
    for label,directory in locations:
        for name in ("config","config.toml"):
            path=directory/name
            if path.is_file():configs[f"{label}/{name}"]=file_hash(path)
    variables={key:value for key,value in env.items() if
        key.startswith(("CARGO_PROFILE_","CARGO_BUILD_","CARGO_TARGET_")) and key!="CARGO_TARGET_DIR" or
        key in ("CARGO_ENCODED_RUSTFLAGS","CARGO_INCREMENTAL","RUSTFLAGS","RUSTDOCFLAGS","RUSTC","RUSTC_WRAPPER","RUSTC_WORKSPACE_WRAPPER")}
    profiles=tomllib.loads((root/"Cargo.toml").read_text(encoding="utf-8")).get("profile",{})
    return dict(workspace_profiles=profiles,cargo_configs=configs,environment=variables)

def tree_hash(paths):
    sha=hashlib.sha256()
    for path in sorted(paths):
        sha.update(str(path.relative_to(ROOT)).encode()+b"\0")
        sha.update(path.read_bytes()+b"\0")
    return sha.hexdigest()

def source_hashes():
    harness=list((ROOT/"dev/bench/src").rglob("*.rs"))+list((ROOT/"dev/bench/benches").rglob("*.rs"))+list((ROOT/"dev/fixtures/src").rglob("*.rs"))
    harness += [ROOT/"tools/bench/run.py",ROOT/"dev/bench/Cargo.toml",ROOT/"dev/fixtures/Cargo.toml",ROOT/"dev/fixtures/assets/cases.json"]
    engine=list((ROOT/"crates/shodo/src").rglob("*.rs"))+[ROOT/"crates/shodo/Cargo.toml",ROOT/"Cargo.toml"]
    return dict(harness_hash=tree_hash(harness),source_hash=tree_hash(engine))

def collect(stage,args):
    """Build once per mode; all measured commands run sequentially."""
    logs=stage/"logs";logs.mkdir()
    bins=stage/"bin";bins.mkdir()
    env=os.environ.copy()
    # An inherited quick/filter/output setting must not silently change the run.
    for key in ("SHODO_BENCH_QUICK","SHODO_BENCH_CASE","SHODO_BENCH_OUTPUT"):
        env.pop(key,None)
    env.update(CARGO_BUILD_JOBS="1",CARGO_PROFILE_RELEASE_OPT_LEVEL="3",CARGO_PROFILE_RELEASE_DEBUG="0",CARGO_PROFILE_RELEASE_INCREMENTAL="false",CARGO_INCREMENTAL="0")
    if "CARGO_TARGET_DIR" not in env:
        common=execute(["git","rev-parse","--git-common-dir"],logs/"git-common.log").strip()
        env["CARGO_TARGET_DIR"]=str((ROOT/common).resolve().parent/"target/performance-harness")
    initial_hashes=source_hashes()
    initial_build=build_configuration(ROOT,env)
    actual_profiles={}
    def command(argv,name):return execute(argv,logs/(name+".log"),env=env)
    cargo=["cargo","+stable"]
    ensure_lock(ROOT,logs/"resolve-lock.log",env)
    default=command(cargo+["build","--locked","--release","-p","shodo-bench","--bin","shodo-probe","--message-format=json"],"build-cold")
    cold_profiles={}
    shutil.copy2(executable_from_cargo(default,"shodo-probe","bin",profiles=cold_profiles),bins/"cold")
    actual_profiles["cold"]=cold_profiles["shodo-probe"]
    benchmark=command(cargo+["build","--locked","--profile","release","-p","shodo-bench","--bench","layout","--message-format=json"],"build-timing")
    timing_profiles={}
    shutil.copy2(executable_from_cargo(benchmark,"layout","bench",profiles=timing_profiles),bins/"layout")
    actual_profiles["timing"]=timing_profiles["layout"]
    instrumented=command(cargo+["build","--locked","--release","-p","shodo-bench","--bin","shodo-probe","--features","allocation-counting","--message-format=json"],"build-memory")
    memory_profiles={}
    shutil.copy2(executable_from_cargo(instrumented,"shodo-probe","bin",profiles=memory_profiles),bins/"memory")
    actual_profiles["memory"]=memory_profiles["shodo-probe"]
    shutil.copy2(ROOT/"Cargo.lock",stage/"Cargo.lock")
    settings=json.loads(command([bins/"cold","--describe"],"describe"))
    selected=[dict(key=f'{s["id"]}/{s["scale"]}',settings=s) for s in settings if args.case is None or s["id"]==args.case]
    keyed(selected)
    raw=stage/"criterion"
    env["SHODO_BENCH_OUTPUT"]=str(raw)
    if args.quick:env["SHODO_BENCH_QUICK"]="1"
    if args.case is not None:env["SHODO_BENCH_CASE"]=args.case
    command([bins/"layout","--bench","--noplot"],"timing")
    timings=load_timings(raw,selected)
    rows=[]
    samples=stage/"samples";samples.mkdir()
    for item in selected:
        settings=item["settings"];key=item["key"]
        cold=[];walls=[]
        for index in range(args.cold_samples):
            before=time.perf_counter_ns()
            output=command([bins/"cold","--cold",settings["id"],str(settings["scale"])],f'cold-{settings["id"]}-{settings["scale"]}-{index}')
            walls.append(time.perf_counter_ns()-before)
            sample=json.loads(output);cold.append(sample)
            (samples/f'cold-{settings["id"]}-{settings["scale"]}-{index}.json').write_text(output,encoding="utf-8")
        output=command([bins/"memory","--memory",settings["id"],str(settings["scale"])],f'memory-{settings["id"]}-{settings["scale"]}')
        memory=json.loads(output)
        (samples/f'memory-{settings["id"]}-{settings["scale"]}.json').write_text(output,encoding="utf-8")
        rows.append(dict(**item,timing=timings[key],cold=cold,process_wall_ns=walls,memory=memory))
    require(source_hashes()==initial_hashes, "source changed while measuring")
    require(build_configuration(ROOT,env)==initial_build, "Cargo configuration changed while measuring")
    require(file_hash(ROOT/"Cargo.lock")==file_hash(stage/"Cargo.lock"), "lockfile changed while measuring")
    cpu=[]
    if Path("/proc/cpuinfo").is_file():
        cpu=sorted({line.split(":",1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")})
    conditions=dict(source_fingerprint_version=SOURCE_FINGERPRINT_VERSION,rustc=command(["rustc","+stable","-Vv"],"rustc").strip(),cargo=command(cargo+["-V"],"cargo").strip(),cpu=dict(models=cpu,machine=platform.machine(),logical_cpus=os.cpu_count(),affinity=sorted(os.sched_getaffinity(0)) if hasattr(os,"sched_getaffinity") else None),os=platform.platform(),features=["complex-scripts"],profile=dict(name="release",opt_level=3,debug=0,incremental=False,effective=actual_profiles),build_configuration=initial_build,flags={k:env.get(k,"") for k in ("RUSTFLAGS","CARGO_ENCODED_RUSTFLAGS","RUSTDOCFLAGS","RUSTC","RUSTC_WRAPPER","RUSTC_WORKSPACE_WRAPPER","CARGO_PROFILE_RELEASE_LTO","CARGO_PROFILE_RELEASE_CODEGEN_UNITS","CARGO_PROFILE_RELEASE_PANIC","CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS","CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS")},font_hashes={p.name:file_hash(p) for p in sorted((ROOT/"dev/fixtures/assets/fonts").iterdir()) if p.is_file()},input_hash=hashlib.sha256(json.dumps(selected,sort_keys=True,separators=(",",":")).encode()).hexdigest(),harness_hash=initial_hashes["harness_hash"],lock_hash=file_hash(stage/"Cargo.lock"),measurement_config=dict(quick=args.quick,cold_samples=args.cold_samples))
    metadata=dict(revision=command(["git","rev-parse","HEAD"],"revision").strip(),dirty_status=command(["git","status","--porcelain"],"status").splitlines(),source_hash=initial_hashes["source_hash"],conditions=conditions,process_wall_scope="Parent subprocess invocation, startup, probe work, JSON output and log write; inner cold phases reported separately.")
    report=dict(schema=1,metadata=metadata,selected=selected,rows=rows)
    validate_report(report)
    (stage/"metadata.json").write_text(json.dumps(metadata,indent=2,allow_nan=False)+"\n",encoding="utf-8")
    if args.baseline:
        baseline=json.loads((Path(args.baseline)/"results.json").read_text())
        comparison=compare(report,baseline)
        (stage/"comparison.json").write_text(json.dumps(comparison,indent=2,allow_nan=False)+"\n",encoding="utf-8")
    return report

def main(argv=None):
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output",required=True,type=Path)
    parser.add_argument("--baseline",type=Path)
    parser.add_argument("--quick",action="store_true")
    parser.add_argument("--case")
    parser.add_argument("--cold-samples",type=int,default=3)
    args=parser.parse_args(argv)
    try:
        validate_config(args.output,args.case,args.cold_samples)
        if args.baseline:validate_report(json.loads((args.baseline/"results.json").read_text()))
        publish(args.output.resolve(),lambda stage:collect(stage,args))
    except (ValueError,OSError,subprocess.CalledProcessError) as error:
        if isinstance(error,subprocess.CalledProcessError):
            for output in (error.stdout,error.stderr):
                if output:
                    print(output,file=sys.stderr,end="")
        print(f"benchmark failed: {error}",file=sys.stderr)
        return 1
    print(f"Saved validated measurements: {args.output}")
    return 0

if __name__=="__main__":
    raise SystemExit(main())
