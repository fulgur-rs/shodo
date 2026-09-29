#!/usr/bin/env python3
"""Classify the frozen S4 residual replay; retain every document/root/value.

This is a report transformation, not a CSS implementation or WPT runner.
Unknown fields require a new ownership decision rather than a guessed category.
"""
import hashlib
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

PIN = "ab7e619a8f321f03de8b8c8b9342954868e044c8"
STYLE_SHA = "4a225616ca97c1b83eeadff6c5228cc23c32ab36a033bb3a4f8d66ecdf4f977f"
API = "existing-api-unwired"
GAP = "caller-feature-gap"
OUT = "legitimate-ifc-scope-rejection"
RULES = {
    "hanging_punctuation": (API, "LineOptions.hanging_punctuation.first", "shodo-9an.1"),
    "cssom_writing_mode": (API, "ParagraphStyle.writing_mode", "shodo-3v2"),
    "text_orientation": (API, "InlineStyle.text_orientation", "shodo-3v2"),
    "text_decoration_line": (API, "PaintStyle/source regions; production decoration propagation", "shodo-0zm"),
    "float": (API, "BFC float placement and Taffy FloatContext and shodo FloatCursor/LineConstraint handoff", "shodo-0zm"),
    "clear": (API, "BFC clear placement before the paragraph", "shodo-0zm"),
    **{field: (GAP, owner, "shodo-0zm") for field, owner in {
        "background_color": "box paint; solid leaf backgrounds already exist in candidate_page",
        "background_image": "box background gradient paint",
        "background_position": "box background geometry",
        "background_repeat": "box background tiling",
        "background_size": "box background geometry",
        "position": "relative box placement; absolute boxes are outside normal IFC flow",
        "left": "positioned box placement",
        "top": "positioned box placement",
        "z_index": "box stacking/paint order",
        "overflow": "box clip/scroll ownership",
        "outline": "box outline paint",
    }.items()},
}


def classify(source):
    if source["raikiri_revision"] != PIN or source["s4_style_source_sha256"] != STYLE_SHA:
        raise ValueError("unreviewed pin or S4 style profile")
    if not source["complete"] or source["input_errors"] != 0:
        raise ValueError("incomplete diagnostic replay")
    fields = defaultdict(lambda: {"blocks": 0, "documents": set(), "values": set(), "initial": set()})
    categories = defaultdict(lambda: {"blocks": 0, "documents": set()})
    doc_groups = Counter()
    profiles = Counter()
    documents, ids, fonts, resources = [], set(), [], {}
    for case in source["cases"]:
        if case["id"] in ids or case["classification"] != "noninitial-style-diagnostic":
            raise ValueError("duplicate or incomplete original document")
        ids.add(case["id"])
        if case["original_font_sha256"] not in fonts:
            fonts.append(case["original_font_sha256"])
        for resource in case["original_resources_verified"]:
            if resource["error"] is not None or not resource["sha256"]:
                raise ValueError("missing original resource evidence")
            key = resource["url"]
            if key in resources and resources[key] != resource:
                raise ValueError("conflicting original resource bytes")
            resources[key] = resource
        blocks, doc_categories = [], set()
        for block in case["blocks"]:
            diffs = block["differences"]
            if not diffs or any(d["field"] not in RULES for d in diffs):
                raise ValueError("unclassified residual field: review its owner first")
            relevant = []
            for ancestor in block["ancestors"]:
                relevant.append(ancestor)
                if ancestor["node"] == block["root"]:
                    break
            if not relevant or relevant[-1]["node"] != block["root"]:
                raise ValueError("error node is outside recorded root")
            out_of_flow = any(a["position"] in ("Absolute", "Fixed") for a in relevant)
            block_categories = {OUT} if out_of_flow else {RULES[d["field"]][0] for d in diffs}
            doc_categories.update(block_categories)
            profiles[block["input_profile"]] += 1
            for category in block_categories:
                categories[category]["blocks"] += 1
                categories[category]["documents"].add(case["id"])
            for diff in diffs:
                field = fields[diff["field"]]
                field["blocks"] += 1
                field["documents"].add(case["id"])
                field["values"].add(diff["value"])
                field["initial"].add(diff["initial"])
            blocks.append({key: block[key] for key in ("root", "root_tag", "error_node", "node_tag", "element_id", "class", "input_profile", "differences")} | {
                "categories": sorted(block_categories),
                "scope_reason": "out-of-flow absolute/fixed box, not an ordinary IFC root/inline" if out_of_flow else "in-flow text input also contains properties owned by the named caller",
                "issues": sorted({RULES[d["field"]][2] for d in diffs}),
            })
        if not blocks:
            raise ValueError("document has no diagnosed blocks")
        doc_groups[tuple(sorted(doc_categories))] += 1
        documents.append({"id": case["id"], "categories": sorted(doc_categories), "blocks": blocks,
                          "resources": [r["url"] for r in case["original_resources_verified"]],
                          "font_registry": fonts.index(case["original_font_sha256"]), "parse_warnings": case["parse_warnings"]})
    actual_blocks = sum(len(c["blocks"]) for c in documents)
    if len(documents) != source["expected_documents"] or actual_blocks != source["expected_blocks"] or actual_blocks != source["reported_blocks"]:
        raise ValueError("original document/block coverage is incomplete")
    return {
        "scope": "all original residual rejections; categories describe caller ownership, not WPT PASS/FAIL",
        "raikiri_revision": PIN, "s4_style_source_sha256": STYLE_SHA,
        "original_comparison_sha256": source["original_comparison_sha256"],
        "viewport_css_px": source["viewport_css_px"],
        "complete": True, "documents": len(documents), "blocks": actual_blocks,
        "candidate_wpt_image_verdicts": 0, "pass_delta": None,
        "input_profiles": dict(sorted(profiles.items())),
        "field_rules": {k: {"category": v[0], "owner": v[1], "issue": v[2]} for k, v in sorted(RULES.items())},
        "fields": {k: {"blocks": v["blocks"], "documents": len(v["documents"]), "values": sorted(v["values"]), "initial": sorted(v["initial"])} for k, v in sorted(fields.items())},
        "categories": {k: {"blocks": v["blocks"], "documents": len(v["documents"])} for k, v in sorted(categories.items())},
        "document_groups": [{"categories": list(k), "documents": v} for k, v in sorted(doc_groups.items())],
        "font_registries": fonts, "original_resources": dict(sorted(resources.items())), "cases": documents,
    }


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: classify_raikiri_styles.py <residual-diagnostics.json> <classified.json>")
    raw = Path(sys.argv[1]).read_bytes()
    result = classify(json.loads(raw))
    result["diagnostic_sha256"] = hashlib.sha256(raw).hexdigest()
    Path(sys.argv[2]).write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"{result['documents']} documents, {result['blocks']} blocks; no unclassified fields")


if __name__ == "__main__":
    main()
