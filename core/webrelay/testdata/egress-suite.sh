#!/bin/sh
# Runs the webrelay network egress suite (TestNetworkEgress in egress_network_test.go).
#
# usage: core/webrelay/testdata/egress-suite.sh <work-dir>
#
# Builds the webrelay test binary with the race detector in a Go container from this checkout,
# then starts the game fixture at 172.30.240.3 and the relay at 172.30.240.2 on an --internal
# Docker network 172.30.240.0/29 with no published ports. It refuses to start when its network
# name exists or another network or route already uses 172.30.240.0/29, and always removes its
# own two containers and network. The test binary and Go caches stay in <work-dir>.
#
# Environment:
#   EGRESS_PREFIX     container and network name prefix (default webrelay-egress)
#   EGRESS_GO_IMAGE   Go image matching core/go.mod
#   EGRESS_GOCACHE    host Go build cache (default <work-dir>/gocache)
#   EGRESS_GOMODCACHE host Go module cache (default <work-dir>/gomodcache)
set -eu

if [ "$#" -ne 1 ]; then
	echo "usage: $0 <work-dir>" >&2
	exit 2
fi
root=$(cd "$(dirname "$0")/../../.." && pwd)
mkdir -p "$1"
work=$(cd "$1" && pwd)
prefix=${EGRESS_PREFIX:-webrelay-egress}
image=${EGRESS_GO_IMAGE:-golang:1.26.1-bookworm@sha256:ab3d6955bbc813a0f3fdf220c1d817dd89c0b3f283777db8ece4a32fe7858edd}
gocache=${EGRESS_GOCACHE:-$work/gocache}
gomodcache=${EGRESS_GOMODCACHE:-$work/gomodcache}
subnet=172.30.240.0/29
game=172.30.240.3:19132
network=$prefix-net
user=$(id -u):$(id -g)

if docker network inspect "$network" >/dev/null 2>&1; then
	echo "network $network already exists" >&2
	exit 2
fi
for id in $(docker network ls -q); do
	if docker network inspect -f '{{range .IPAM.Config}}{{.Subnet}} {{end}}' "$id" | grep -q '172\.30\.240\.'; then
		echo "another Docker network already uses $subnet" >&2
		exit 2
	fi
done
if ip route | grep -q '172\.30\.240\.'; then
	echo "a route already covers $subnet" >&2
	exit 2
fi

mkdir -p "$gocache" "$gomodcache"
docker run --rm --name "$prefix-build" --user "$user" \
	-e HOME=/tmp -e GOCACHE=/cache/build -e GOMODCACHE=/cache/mod -e GOTOOLCHAIN=local -e GOFLAGS=-mod=readonly \
	-v "$gocache":/cache/build -v "$gomodcache":/cache/mod -v "$work":/out -v "$root":/src:ro -w /src/core \
	"$image" go test -c -race -o /out/webrelay.test ./webrelay

cleanup() {
	docker rm -f "$prefix-game" "$prefix-relay" >/dev/null 2>&1 || true
	docker network rm "$network" >/dev/null 2>&1 || true
}
trap cleanup EXIT
docker network create --internal --subnet "$subnet" "$network" >/dev/null
docker run -d --name "$prefix-game" --network "$network" --ip 172.30.240.3 --user "$user" \
	-e HOME=/tmp -e WEBRELAY_EGRESS_ROLE=game -e WEBRELAY_EGRESS_GAME="$game" \
	-v "$work":/suite:ro -w /tmp "$image" \
	/suite/webrelay.test -test.run '^TestNetworkEgressGameFixture$' -test.timeout 10m -test.v >/dev/null
status=0
docker run --rm --name "$prefix-relay" --network "$network" --ip 172.30.240.2 --user "$user" \
	-e HOME=/tmp -e WEBRELAY_EGRESS_ROLE=relay -e WEBRELAY_EGRESS_GAME="$game" \
	-e WEBRELAY_EGRESS_GAME_NAME="$prefix-game" -e WEBRELAY_EGRESS_SELF=172.30.240.2 \
	-v "$work":/suite:ro -w /tmp "$image" \
	/suite/webrelay.test -test.run '^TestNetworkEgress$' -test.timeout 8m -test.v || status=$?
echo "--- game fixture"
docker logs "$prefix-game" 2>&1 | tail -20
exit "$status"
