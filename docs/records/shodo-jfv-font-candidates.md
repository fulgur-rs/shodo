# shodo-jfv: 登録済みフォント候補の準備共有

2026-10-09。基準main `73559b25`（#270後）、製品・probe commit `fcdfb46801470c779005dcebe726007f1f9743b4`。詳細値・条件・source/binary hashは同名JSONに保存。

## 変更と採否

登録済みフォントのCandidate metadataとwidth/style/weightの属性順位を、レイヤーごとのfamily＋正規化属性で再利用する。named familyは属性の最良群へ絞ってから文字coverageを調べる。last resortは別キーで全属性群を対象にする。文字ごとのunicode-range・cluster coverage・VS15/VS16/Autoのcolor preferenceと、同順位のorder_cmpを維持する。synthesis・variationは選択後に現在のqueryから計算する。native候補のload/materializationは従来経路のまま。

公開optionの追加はない。`match_cache_entries=0`でcluster matchと今回の準備共有の両方を無効にする。共有は最大16指定、候補総数256、追加の所有heap保持64 KiB、familyキー4,096 bytes。descriptorのfamily/range容量、FontInfo axesの保守的容量、Arc・slots・キーを計上し、catalogが既に所有する共有blob/pathのbytesは追加保持へ重複計上しない。キャッシュの固定inline状態もレイヤーごとに増える。巨大な入力は変換前に保守的に判定し、従来のbest_matchへ戻す。namedの属性絞り込みで最終候補数が小さくなる入力でも、入力側の見積もりが大きければ共有しない。

上限を超えるとLRU eviction。stampはfetch_maxで並行hitにも単調に更新する。フォント登録・親/子世代・generic/fallback変更をgeneration keyに反映し、次の候補lookupで古いpayloadを解放する。collection破棄でも解放する。FontCollectionに公開shrink APIはなく、今回も追加していない。cacheとcatalogのguardは同時保持しない。準備中にgenerationが変わったpayloadは挿入しない。

採用理由は、異種文字での候補準備・割当・命令数削減を、保持量に上限を付けて実現できたこと。全入力での速度改善は主張しない。単一候補のbuildと既存match-cache hit中心の対照では時間が悪化した計測もあり、後述の条件と限界を含めて評価する。

## 操作回数と選択契約

4同順位候補×異なる3文字（水・日・本）のテストは、変更前のregistered_face_visit=12で期待4に対してRED、変更後は4でGREEN。準備時の属性順位計算は4回、font readは12回のまま。coverageを省く変更ではない。従来の最終選択rank observerは属性絞り込み中の計算を数えないので、それを全rank処理量として扱わない。

新規7テストは、LRU/ASCII family/無効化、両レイヤーのgenerationとWeakによるpayload解放、件数・候補・bytes上限、巨大キー/候補/rangeの保持拒否、属性・unicode-range・coverage・last resortの従来処理との結果一致、CSS登録color/text・VS・synthesis切替を確認する。variable fontのweight/width/style、variationも比較する。既存のfamily/fallback順・native・親子・generic/locale検証を含むfont suiteは126→133件。

## 条件と測定範囲

AMD Ryzen 5 5600G、Linux 7.2.5-3-omarchy、Rust 1.91.0 / LLVM 21.1.2。固定CJK実フォント `d8b52a1d…`、system fontsなし、Limits unlimited。同familyへ400/700のdescriptorを交互に登録する。native対照はdescriptorなしのregisterでintrinsic familyを指定する。同一probeを前後でrelease build、debug=0、incremental=false。時間用とallocation-counting用のバイナリを分離する。

- matching: capacity 8、38 scalarの日本語列（文字種は重複もある）と日38文字の反復対照。初回cold lookup＋継続reuse、各20反復。run_matches内の出力Vec確保・途中のFontMatch破棄を含み、最後のVec破棄とfont登録は範囲外。working setがmatch cacheを超える条件を意図的に測る。
- build: 既定capacity 256、上の列×32＝1,216 scalar。毎回新規document/Context、共有rootは保持し、初回cold準備後はroot cacheを再利用する。各20反復。document構築と途中のParagraph/Context破棄を含み、最後のParagraph/Context破棄・行分割・digest・font登録は範囲外。既存document内のwarm paragraph build全般を代表する測定ではない。

CallgrindはホストValgrind 3.25.1、collect-atstart=no、toggle-collect=*run_matches*／*run_builds*、cache/branch simulationなし。全24 processでrunのinclusive IrとPROGRAM TOTALSが一致。IrはCPU時間ではない。

## Callgrind・割当・保持

保持増はallocation scopeのnet差、peak増はpeak_extra_bytes差。matchingのscope終了時はfont collectionと最後の出力Vec、buildではrootと最後のParagraph/Contextが生存し、準備cache単独の保持量ではない。共通の登録blob bytesは計測開始前のliveに含まれる。requested bytesでありRSSではない。

| 経路 / 候補数 / 入力 | Ir 後/前 | 割当回数 前→後 | net差 bytes | peak差 bytes |
|---|---:|---:|---:|---:|
| matching / 1 / distinct | 0.764732 | 8,928 → 6,481 | +796 | +1,488 |
| matching / 16 / distinct | 0.545737 | 21,828 → 6,514 | +6,516 | +6,412 |
| matching / 128 / distinct | 0.480334 | 118,148 → 6,744 | +50,196 | +49,644 |
| matching / 512 / distinct | 1.002805 | 448,388 → 448,388 | +0 | +0 |
| matching / 16 / repeat | 1.002554 | 1,550 → 1,557 | +2,316 | +4,092 |
| matching / 16 / native | 0.995032 | 20,768 → 19,917 | +4,385 | +4,385 |
| build / 1 / distinct | 0.993351 | 77,021 → 73,774 | +1,128 | +1,460 |
| build / 16 / distinct | 0.956886 | 90,521 → 73,807 | +6,848 | +7,180 |
| build / 128 / distinct | 0.776493 | 191,321 → 74,037 | +50,528 | +50,860 |
| build / 512 / distinct | 1.001453 | 536,921 → 536,241 | +332 | +664 |
| build / 16 / repeat | 0.989225 | 8,681 → 6,886 | +2,608 | +2,900 |
| build / 16 / native | 0.997689 | 89,501 → 87,930 | +4,730 | +5,075 |

初期実装は512候補を共有形式へ変換した後に上限で捨て、Ir比1.233433だった。事前判定へ修正し、最終matching比1.002805・割当増0となった。これは破棄した途中版の値で、採用版の表とは分ける。

## 実時間

他の本作業のbuild/Valgrind実行後にCPU 2固定、ABBA 4 round＋BAAB 4 round。各processのnはJSONに保存。初回coldを含むloop全体の時間を取り、round内の後2回平均/前2回平均の中央値を示す。CPUの周波数・SMT sibling CPU 8・外部processは制御していない。

| 経路 / 候補数 / 入力 | 後/前中央値 | 短縮round | round比の範囲 |
|---|---:|---:|---:|
| time / 1 / distinct | 0.7289 | 8/8 | 0.692–0.805 |
| time / 16 / distinct | 0.5012 | 8/8 | 0.458–0.614 |
| time / 128 / distinct | 0.4787 | 8/8 | 0.353–0.700 |
| time / 512 / distinct | 1.1267 | 2/8 | 0.702–1.253 |
| time / 16 / repeat | 1.0527 | 0/8 | 1.010–1.292 |
| time / 16 / native | 0.9831 | 6/8 | 0.823–1.169 |
| build-time / 1 / distinct | 1.0347 | 2/8 | 0.782–1.135 |
| build-time / 16 / distinct | 0.9037 | 6/8 | 0.644–1.256 |
| build-time / 128 / distinct | 0.7573 | 8/8 | 0.690–0.779 |
| build-time / 512 / distinct | 1.0163 | 2/8 | 0.978–1.089 |
| build-time / 16 / repeat | 0.9680 | 8/8 | 0.957–0.981 |
| build-time / 16 / native | 1.0060 | 2/8 | 0.944–1.025 |

異種文字matchingは各候補数1/16/128で全round短縮し、buildの128候補も全round短縮。一方、単一候補buildは中央値約3.5%増、同一文字matchingは約5.3%増で全round増加した。後者のIrは+0.26%に留まるが、実時間の悪化を無退行へ読み替えない。512候補ではmatching中央値+12.7%とばらつきがあり、native対照もround間で変動した。冷たい候補準備の削減とbounded retentionを評価しつつ、warm hit中心・単一候補の速度が重要な用途では静かな環境と対象corpusで追加評価する。

## Valgrindメモリ

Massifはホスト3.25.1、stacks=no/time-unit=B/detailed-freq=1/max-snapshots=200、各20反復。font登録・行分割・出力・終了まで含む全processで、requested heapとallocator overheadを含むtotal peakを分けた。

| 経路 / 候補数 | requested peak 前→後 bytes | total peak 前→後 bytes |
|---|---:|---:|
| loop / 1 | 223,238 → 224,090 | 225,680 → 228,272 |
| loop / 128 | 27,016,699 → 27,036,995 | 27,023,400 → 27,063,936 |
| loop / 512 | 108,044,263 → 108,044,319 | 108,076,304 → 108,076,368 |
| build-loop / 16 | 4,510,240 → 4,517,476 | 4,582,728 → 4,593,080 |

ホストglibc 2.44では、前件で確認済みのローダーmandatory memcmp redirection用debug情報不足がある。ホストを変更せず、既存Debian bookworm-slim imageから--rmコンテナを作り、内部だけでvalgrind/libc6-dbgを導入してMemcheckした。Valgrind 3.19.0 / glibc 2.36-9+deb12u14、同じホストbuildバイナリで、libc/libgccはコンテナのもの。ホストglibcでのMemcheckとは区別する。image digest・options・package版はJSONに保存。

各側でmatchingの1/128候補×20反復・512候補×2反復、buildの16候補×2反復、計8 process。全てexit=0、ERROR SUMMARY=0、suppression=0、definitely/indirectly/possibly lost=0。前後digestも一致。still reachableは両版456 bytes/1 blockで、全backtraceはRust stack_overflow::thread_info::set_current_info。cache owner終了後のlost分類リークは検出されなかった。任意入力のメモリ安全性の証明とはしない。コンテナは--rmで削除済み。

## 検証

- workspace: 1,729 passed、AccessKit: 1,745 passed。各9 ignoredは既存。
- shodo no-default: 1,194 passed、complex-scripts併用: 1,197 passed。各9 ignored。
- release font suite: 133 passed。allocation tests: 18 passed。
- fmt/diffチェック、全target strict Clippy（通常/AccessKit、最終probe含む）、strict rustdoc（通常/AccessKit）。
- 固定snapshot46件、全match、changed_pixels=0、更新なし。matching/build/Valgrindの前後digest一致。
- 独立レビューの未解決指摘なし。並行hitのrecency stamp指摘はfetch_maxで修正、上限超過のfallbackも再レビュー済み。

## 再現と後片付け

一時出力は~/tmpに作る。比較対象の2 checkoutにこのcommitの同一probeとdev/bench manifestを置き、別targetへbuildし、feature統合を避けてtime/alloc実行ファイルをそれぞれ保存する。以下は各checkoutで実行する例。

```sh
mkdir -p "$HOME/tmp"
measure_dir=$(mktemp -d -p "$HOME/tmp" shodo-jfv.XXXXXXXX)
export TMPDIR="$measure_dir" CARGO_TARGET_DIR="$measure_dir/target"
export CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_INCREMENTAL=false
unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER
cargo build --release -p shodo-bench --example font_candidate_prep
cp "$CARGO_TARGET_DIR/release/examples/font_candidate_prep" "$measure_dir/probe-time"
cargo build --release -p shodo-bench --example font_candidate_prep --features allocation-counting
cp "$CARGO_TARGET_DIR/release/examples/font_candidate_prep" "$measure_dir/probe-alloc"
"$measure_dir/probe-alloc" alloc 16 distinct 20
"$measure_dir/probe-alloc" build-alloc 16 distinct 20
valgrind --tool=callgrind --collect-atstart=no --toggle-collect="*run_matches*" --cache-sim=no --branch-sim=no --callgrind-out-file="$measure_dir/callgrind.out" "$measure_dir/probe-time" loop 16 distinct 20
taskset -c 2 "$measure_dir/probe-time" build-time 16 distinct 100
```

build Callgrindはtoggleを*run_builds*、modeをbuild-loopへ切り替える。facesは1/16/128/512、入力はdistinct/repeat/native。timeは上の表とJSONのnを使い両版をABBA/BAABで実行する。Massifは上記optionsでprobe-timeのloop/build-loopを実行。MemcheckはJSONのimage digestで--rmコンテナを使い、measure_dirをmountしてTMPDIRとlog-fileをその中へ向ける。

正式記録・PRへの集計保存と最終確認後、この作業の~/tmp/shodo-jfv.*・baseline比較worktreeだけを削除する。PRが開いている間は実装worktree `.claude/worktrees/shodo-jfv` をレビュー対応用に保持する。root mainのユーザー作業14 untracked filesは変更しない。
