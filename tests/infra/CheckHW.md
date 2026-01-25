# Check Hardware

Get software driver
```sh
ethtool -i eth0
```

Get hardware
```sh
lspci
```

```sh
# Find the VF interface name (usually enP* or something similar)
find /sys/class/net -name "lower_*" -exec basename {} \; 2>/dev/null | sed 's/lower_//'

# Or check netvsc bonding
cat /sys/class/net/eth0/bonding_slave/state 2>/dev/null

# Detailed view of all net devices and their drivers
for dev in /sys/class/net/*/device/driver; do
  iface=$(echo $dev | cut -d'/' -f5)
  driver=$(basename $(readlink $dev))
  echo "$iface: $driver"
done
```