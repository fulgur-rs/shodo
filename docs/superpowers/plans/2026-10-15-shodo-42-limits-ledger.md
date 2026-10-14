# SHODO-42 Limits Ledger Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 常時有効な一つの階層型台帳で段落と ruby の資源制限を維持する。

**Architecture:** owned ledger に scope と frame を格納し producer が直接予約する。試行会計は checkpoint / rollback / commit で確定する。

**Tech Stack:** Rust、既存 cargo test。

**Spec:** `docs/superpowers/specs/2026-10-15-shodo-42-limits-ledger-design.md`

## Global Constraints

- 公開 Limits / LimitExceeded と警告の順序を維持する。
- processed items と shaping items は別の counter を同じ Items cap と照合する。
- Send/Sync を維持し、line ファイルは SHODO-41 の担当に委ねる。
- 一時ファイルは ~/tmp、ビルド成果物は指定された共有 target に格納する。

## Review Focus

- nested annotation の費用が全祖先に累積する。
- sibling base の予算が互いに流用されない。
- 残量が等しい cap でも original cap と actual を正しく報告する。
- first-line の複製予約が replay と一致する。
- failed/unselected trial が会計と warning を汚さない。

### Task 1: Ledger と所有 frame

**Files:** Create `crates/shodo/src/limits/ledger.rs`; modify `limits.rs`; retire `ruby/budget.rs`, `ruby/base_budget.rs`.
**Interfaces:** `LimitsLedger::new(&Limits)`, frame registration/switch, owner charge, transaction checkpoint/rollback/commit。

- [x] nested/sibling/equal-room/rollback の failing tests を書く。
- [x] テストを実行して失敗を確認する。
- [x] checked, saturating, owner-first 会計を実装する。
- [x] ledger テストを実行する。

### Task 2: Analysis producer と paragraph build

**Files:** `analysis/{whitespace,transform,itemize}.rs`, `mapping.rs`, `paragraph/{build.rs,build/analysis_cost.rs,build/paint_reuse_tests.rs}`, `paragraph.rs`.
**Interfaces:** 全 producer が `&mut LimitsLedger` を受け取り、mapping/analysis cost は使用統計を返す。

- [x] equal-room first-line/root/base の回帰テストを追加する。
- [x] 残量 Limits コピー、エラー書換え、enabled 分岐、scoped twin を削除する。
- [x] producer reservation を clone/replay にも適用する。
- [x] analysis と mapping、paint reuse の既存テストを実行する。

### Task 3: Shaping と ruby preparation

**Files:** `shape.rs`, `shape/tests.rs`, `ruby/{prepare,index,mod}.rs`.
**Interfaces:** checkpoint 会計を持つ width trial と `commit`、container scope の annotation frame。

- [x] failed trial / selected trial / exact glyph boundary のテストを維持・追加する。
- [x] WindowCharge と failure 再現用 re-shaping を削除する。
- [x] ruby metadata/cut/index producer を直接課金する。
- [x] TCY と ruby limit tests を実行する。

### Task 4: Verification と integration handoff

- [x] fmt、対象 crate の test と clippy を実行する。
- [x] stale accounting path を検索し、公開 API と warning order を確認する。
- [x] 担当ファイルだけを commit し、統合担当に証拠と変更内容を渡す。
