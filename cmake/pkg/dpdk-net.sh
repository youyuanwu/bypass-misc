#!/bin/sh
# Set up environment for DPDK installed in /opt/dpdk
# NOTE: Commented out - using default /usr/local prefix which is in default search paths

# _dpdk_pkgconfig="/opt/dpdk/lib/x86_64-linux-gnu/pkgconfig"
# case ":${PKG_CONFIG_PATH}:" in
#     *":${_dpdk_pkgconfig}:"*) ;;
#     *) export PKG_CONFIG_PATH="${_dpdk_pkgconfig}${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" ;;
# esac
# unset _dpdk_pkgconfig

# _dpdk_bin="/opt/dpdk/bin"
# case ":${PATH}:" in
#     *":${_dpdk_bin}:"*) ;;
#     *) export PATH="${_dpdk_bin}${PATH:+:$PATH}" ;;
# esac
# unset _dpdk_bin
