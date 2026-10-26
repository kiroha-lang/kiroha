# Codex 引き継ぎ: MySQL/MariaDB backend 対応

## ゴール
MySQL/MariaDB を PostgreSQL と同じ database 抽象で扱えるようにする。**実装・実サーバー検証は完了。コミット前。**

## 現在地
- 作業場所: `/home/mizushima/dev/kiroha`(`/home/mizushima/dev/kiri` のリンク worktree)
- ブランチ: `feat/database-backends`(HEAD `6b4a0ca`、**未コミット**)
- 変更は全て作業ツリー上。`git status` で確認できる。

### 変更内容
- 新規: `src/database.rs`(`Backend`/`SqlType`/`SqlParam`/`RawRows`)
- 新規: `src/host/real/database.rs`(`Connection` トレイトとプール・lease・tx 状態)
- 新規: `src/host/real/mysql.rs`(mysql_async アダプタ。`KILL QUERY` で cancel)
- 新規: `src/sqlcheck/{postgres,mysql}.rs`(`sqlcheck.rs` は振り分けのみ)
- 新規: `tests/database_backends.rs`
- 変更: `pg.rs`(oid→`SqlType` 化、共通プールへ移行)、`builtins/db.rs`、`host/config.rs`(pool に `backend`: `postgresql`(既定)/`mysql`/`mariadb`)、`virtual_host.rs`ほか
- 文書/CI: `README.md`、`docs/architecture.md`、`plugins/.../references/host-config.md`、`.github/workflows/verify.yml`(mysql:8.4 と mariadb:11.4 サービス追加)

## 検証済み(実サーバー、全 14 テストバイナリ通過、SKIP 0)
MySQL 8.4.4 / MariaDB 11.4.5 / PostgreSQL 16.4。Clippy 警告ゼロ。

テスト用 DSN:
```sh
KIROHA_TEST_PG_DSN='host=127.0.0.1 port=55433 user=kiroha dbname=postgres' \
KIROHA_TEST_MYSQL_DSN='mysql://e@127.0.0.1:33062/kiroha_t' \
KIROHA_TEST_MARIADB_DSN='mariadb://e@127.0.0.1:33061/kiroha_t' cargo test --no-fail-fast
```
DSN 未設定のテストは `SKIP` と出て**通ってしまう**ので、SKIP が 0 か必ず確認すること。

## ローカル DB の再現
Docker も root も無い環境。公式バイナリをユーザー権限で展開して動かした(停止済み)。
- MariaDB: `archive.mariadb.org` の `mariadb-11.4.5-linux-systemd-x86_64.tar.gz`
- MySQL: `cdn.mysql.com/archives/mysql-8.4/mysql-8.4.4-linux-glibc2.28-x86_64.tar.xz`。`libaio.so.1` が無いので `apt-get download libaio1` を `dpkg -x` し、`LD_LIBRARY_PATH` を通す
- PostgreSQL: maven の `io.zonky.test.postgres:embedded-postgres-binaries-linux-amd64:16.4.0` の jar 内 txz
- 展開物は `/tmp/claude-1000/-home-mizushima-dev-kiroha/*/scratchpad/` にある(消えていたら再取得)
- 各サーバーに DB `kiroha_t` と、全権限のユーザーを作る。空パスワードユーザー `e`@`%` は CI の root 空パスワード経路の確認用

## 注意(必読)
- **`git worktree prune` を絶対に打たない。** この worktree の裏ポインタは古く、prune すると自分の管理ディレクトリ(`kiri/.git/worktrees/kiroha-databases`)が消えて git が全滅する。実際に起きて復旧済み。再発したら `HEAD`(`ref: refs/heads/feat/database-backends`)、`commondir`(`../..`)、`gitdir`(`/home/mizushima/dev/kiroha/.git`)の 3 ファイルを作り直して `git reset`。
- `tests/i06_i07_i08.rs::i07_cpu_bound…` は負荷で時間制限(1.5s/5s)が揺れて落ちることがある。素の HEAD では通る。単体で再実行して確認する。
- `spec.md` は measure(`scripts/measure.py --check`、`tiktoken==0.14.0` 必須、未導入)で成果物が管理されている。**編集したら再生成が必要。**
- 仮想ホスト(適合テスト)は PG の `{oid,value}` 契約のまま。MySQL でも oid 経由で変換される。

## 未完・未検証
1. **コミットと PR**(コミット規約は `<type>: <description>`)。まだ何もコミットしていない。
2. **CI の mysql/mariadb サービスは未検証**(ローカルで再現不可)。push して実際に通るか確認する。
3. **`spec.md` §11 が PG 専用のまま**で、実装の `backend` フィールドと食い違う。MySQL/MariaDB プロファイル追加と measure 再生成が必要。方針は要確認。
4. MySQL 版の「COMMIT 応答ロスト」テストが無い(PG は `i04_lost_commit_acknowledgment…`)。

## 次バージョン候補(優先順)
spec §11 への MySQL/MariaDB 追記 → 適合テスト I03/I04 の backend 横断化(`SqlType` ベース)→ COMMIT 応答ロストテスト → float/date/time の厳密な型対応(今は `decode` で拒否)→ SQLite 追加 → 3 DB のローカル起動を `scripts/` 化、`i07` の安定化。
