#!/usr/bin/env bash
# Create deterministic fixture files and start the test servers.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p "fixtures/Movies/Alien (1979)" "fixtures/Shows/Firefly/Season 1"
python3 - <<'PY'
import random, os
def blob(path, size, seed):
    r = random.Random(seed)
    with open(path, "wb") as f:
        f.write(bytes(r.getrandbits(8) for _ in range(size)))
blob("fixtures/Movies/Alien (1979)/Alien.1979.1080p.mkv", 3 * 1024 * 1024 + 123, 1)
blob("fixtures/Movies/Alien (1979)/Alien.1979.1080p.en.srt", 2048, 2)
blob("fixtures/Shows/Firefly/Season 1/Firefly.S01E01.mkv", 512 * 1024, 3)
blob("fixtures/Shows/Firefly/Season 1/Firefly.S01E02.mkv", 700 * 1024 + 7, 4)
open("fixtures/notes.txt", "w").write("not a video\n")
PY
# SFTP key login: two throwaway Ed25519 keys (one with the passphrase "coyotl-key"), made
# fresh on each machine and never committed.
mkdir -p keys
[ -f keys/id_ed25519 ] || ssh-keygen -q -t ed25519 -N '' -C coyotl-test -f keys/id_ed25519
[ -f keys/id_ed25519_pass ] || ssh-keygen -q -t ed25519 -N 'coyotl-key' -C coyotl-test-pass -f keys/id_ed25519_pass
cat keys/id_ed25519.pub keys/id_ed25519_pass.pub > keys/authorized_keys
# Issue #151: the writable share; each test works in a folder of its own.
mkdir -p writable && chmod 0777 writable
docker compose up -d --build --wait 2>/dev/null || docker compose up -d --build
# Wait until every service accepts connections (CI starts the tests right after this).
for hp in 172.29.0.10:445 172.29.0.11:2049 172.29.0.12:8080 172.29.0.12:21 172.29.0.12:2022 172.29.0.12:8081 172.29.0.12:2023 172.29.0.12:2121; do
  for _ in $(seq 60); do
    (exec 3<>"/dev/tcp/${hp%:*}/${hp#*:}") 2>/dev/null && break
    sleep 1
  done
done
echo "test servers: smb://172.29.0.10/media nfs://172.29.0.11/media webdav http://172.29.0.12:8080 sftp :2022 ftp :21"
