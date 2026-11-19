#!/bin/sh
# Runs the webrelay network egress suite (TestNetworkEgress in egress_network_test.go).
#
# usage: core/webrelay/testdata/egress-suite.sh <work-dir>
#
# Builds the webrelay test binary with the race detector in a Go container from this checkout,
# then starts the game fixture at 172.30.240.3 and the relay at 172.30.240.2 on an --internal
# Docker network 172.30.240.0/29 with no published ports. It refuses to start when its network
# name exists or another network or route already uses 172.30.240.0/29. On every exit, including
# SIGINT, SIGTERM and SIGHUP, it removes the containers and network this run created and nothing
# else: each carries a label with this run's random token. The test binary and Go caches stay in
# <work-dir>.
#
# Environment:
#   EGRESS_PREFIX     container and network name prefix (default webrelay-egress-<pid>)
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
prefix=${EGRESS_PREFIX:-webrelay-egress-$$}
image=${EGRESS_GO_IMAGE:-golang:1.26.1-bookworm@sha256:ab3d6955bbc813a0f3fdf220c1d817dd89c0b3f283777db8ece4a32fe7858edd}
gocache=${EGRESS_GOCACHE:-$work/gocache}
gomodcache=${EGRESS_GOMODCACHE:-$work/gomodcache}
subnet=172.30.240.0/29
game=172.30.240.3:19132
user=$(id -u):$(id -g)

# Every container and network this run creates carries its token, so cleanup removes exactly
# those, even one whose create was interrupted before the script saw its ID.
token=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
label=webrelay-egress-run=$token
cleanup() {
	trap '' INT TERM HUP
	for id in $(docker ps -aq --filter "label=$label"); do
		docker rm -f "$id" >/dev/null 2>&1 || true
	done
	for id in $(docker network ls -q --filter "label=$label"); do
		docker network rm "$id" >/dev/null 2>&1 || true
	done
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

# create makes a container labelled with this run's token; a name that already exists fails and
# leaves the other container untouched.
create() {
	id=$(docker create --label "$label" "$@")
}

# attach streams a created container until it exits and returns its status. It waits in the
# background, so a trapped signal ends the wait at once instead of after the container.
attach() {
	docker start -a "$1" &
	wait "$!"
}

if docker network inspect "$prefix-net" >/dev/null 2>&1; then
	echo "network $prefix-net already exists" >&2
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
create --name "$prefix-build" --user "$user" \
	-e HOME=/tmp -e GOCACHE=/cache/build -e GOMODCACHE=/cache/mod -e GOTOOLCHAIN=local -e GOFLAGS=-mod=readonly \
	-v "$gocache":/cache/build -v "$gomodcache":/cache/mod -v "$work":/out -v "$root":/src:ro -w /src/core \
	"$image" go test -c -race -o /out/webrelay.test ./webrelay
build=$id
attach "$build"

network=$(docker network create --label "$label" --internal --subnet "$subnet" "$prefix-net")
create --name "$prefix-game" --network "$network" --ip 172.30.240.3 --user "$user" \
	-e HOME=/tmp -e WEBRELAY_EGRESS_ROLE=game -e WEBRELAY_EGRESS_GAME="$game" \
	-v "$work":/suite:ro -w /tmp "$image" \
	/suite/webrelay.test -test.run '^TestNetworkEgressGameFixture$' -test.timeout 10m -test.v
fixture=$id
docker start "$fixture" >/dev/null
create --name "$prefix-relay" --network "$network" --ip 172.30.240.2 --user "$user" \
	-e HOME=/tmp -e WEBRELAY_EGRESS_ROLE=relay -e WEBRELAY_EGRESS_GAME="$game" \
	-e WEBRELAY_EGRESS_GAME_NAME="$prefix-game" -e WEBRELAY_EGRESS_SELF=172.30.240.2 \
	-v "$work":/suite:ro -w /tmp "$image" \
	/suite/webrelay.test -test.run '^TestNetworkEgress$' -test.timeout 8m -test.v
status=0
attach "$id" || status=$?
echo "--- game fixture"
docker logs "$fixture" 2>&1 | tail -20
exit "$status"
