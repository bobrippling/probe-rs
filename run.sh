#!/bin/sh

# core0: net-debug
# core1: user code
#
# probe-rs/targets/RP2040.yaml is setup so that RP2040_SELFDEBUG_TARGET_SELECT only inspects core1
#
# we then use modified probe-rs to also flash & run our code and attach to core0 using RP2040_CORE0
# (gone with psel just to be sure, so RP2040_CORE0 - .cargo/config.toml)

set -e

usage(){
	echo >&2 "Usage: $0 wire"
	echo >&2 "Usage: $0 net [ip]"
	exit 2
}

if test $# -eq 0
then usage
fi

cd "$(dirname "$0")"

net_debug_dir=../embassy-net-rp-self-debug/

case "$1" in
	wire)
		shift
		if test $# -ne 0; then usage; fi

		cd "$net_debug_dir"

		#	.cargo/config.toml has:
		#	`--chip RP2040_CORE0`
		#	use of the recently built .../debug/probe-rs

		if test -z "$DEFMT_LOG"
		then
			export DEFMT_LOG=info #,embassy_net_rp_self_debug=trace
			echo "$0: DEFMT_LOG defaulted to \"$DEFMT_LOG\""
		fi
		export RUST_LOG=probe_rs::flashing=info,warn
		cargo run \
			--bin embassy-net-rp-self-debug \
			--release
		;;

	net)
		shift
		if test $# -ne 1; then usage; fi
		ip="$1"

		make -C "$net_debug_dir" algo.yaml
		trap 'rm -f /tmp/RP2040.yaml' EXIT
		awk 'BEGIN{p=1}/^- name: nop_ipc/{p=0}{if(p)print}' < probe-rs/targets/RP2040.yaml >/tmp/RP2040.yaml
		cat "$net_debug_dir"/algo.yaml >>/tmp/RP2040.yaml
		mv /tmp/RP2040.yaml probe-rs/targets/RP2040.yaml

		arm_bin="$net_debug_dir/target/thumbv6m-none-eabi/debug/embassy-net-rp-self-debug"

		cargo run \
			run \
			--probe "0:0:$ip:1234" \
			--chip RP2040_SELFDEBUG_TARGET_SELECT \
			--log-file log-run.txt \
			"$arm_bin"

		;;
	*)
		usage
		;;
esac
