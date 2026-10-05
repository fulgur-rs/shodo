# Engine source fingerprintの修正（shodo-sbp.2）

2026年9月30日。performance runnerのsource hashに、移設後の `crates/shodo/src/**/*.rs` と `crates/shodo/Cargo.toml` を含める。workspace manifestも引き続き含め、相対パスと内容からhashするため、ファイルの追加・削除・編集でfingerprintが変わる。生成物やRust以外の補助ファイルは対象に含めない。

測定条件に `source_fingerprint_version: 2` を追加した。古い対象範囲で作成したreport、未知のversion、不正なengine SHA256を拒否する。同じcoverage・測定条件での意図的なengine変更はA/B比較できる。測定開始・終了のhash照合方式は維持し、終了時に残っている実装変更は成果物の公開前に拒否する。途中の編集が終了までに完全に戻された場合を検出する監視方式は、この変更には含めない。

旧実装に追加テストを適用すると11件のassertionが失敗した。修正後はrunner全26テストが成功した。回帰テストでは実際に小さなRust probeとbenchをrelease buildして収集し、正常時の公開成功と、子processがengine sourceを変更した場合の公開拒否・stage削除を検証した。内容・追加削除・crate/workspace manifest・生成物除外・coverage互換性・SHA256形式も検証している。

規定のfontTools 4.61.1環境でtoolsのPython全91テストが成功し、skipは0。fixture再生成checkも成功し、固定assetや期待画像の変更はない。独立コードレビューで指摘はなかった。

CPU affinity 10、stable Rust 1.97.1、release profileで全54 workloadを収集した。378 warm操作、108 cold process、54 memory processすべてについてrunnerの出力・条件・counter整合検証が成功した。収集後のengine hashはreportと一致し、旧shodo-sbp.1 reportは `missing conditions` として拒否された。binary三種、Cargo.lock、toolchain・profile・font・input・harness情報は通常の成果物に保持されている。

検証manifest にengine対象全ファイルとbinary/lockのSHA256、測定条件を記録し、生データarchive に全matrixとred/green・全Python・fixture検証ログを保存した。元成果物は `target/performance-artifacts/sbp2-fingerprint`。測定時点のbaseは `dce8815d8b29e939446368faae99bf4be370a88c` で、runner修正は未コミットだった。

Rust engineの動作は変更していない。計測中に別の検証buildが一部並行したため、この時間値だけから速度改善率を主張しない。raikiri切り替えへの性能ブロック依存や、保存済みS4 spikeへの変更はない。
