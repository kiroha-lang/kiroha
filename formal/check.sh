#!/usr/bin/env sh
# Model-check the TLA+ specifications with TLC.
# Downloads tla2tools.jar (TLA+ 1.8.0) into formal/.tools on first use.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
jar="$here/.tools/tla2tools.jar"
if [ ! -f "$jar" ]; then
  mkdir -p "$here/.tools"
  curl -sSL -o "$jar" https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar
fi
cd "$here"
status=0
for spec in Parallel Tx; do
  echo "== $spec"
  if java -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers auto -deadlock -cleanup "$spec" > ".tools/$spec.log" 2>&1 \
     && grep -q "No error has been found" ".tools/$spec.log"; then
    grep -E "distinct states found" ".tools/$spec.log"
    echo "   OK"
  else
    tail -30 ".tools/$spec.log"
    status=1
  fi
done
exit $status
