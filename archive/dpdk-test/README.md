# DPDK
build from source
```sh
sudo apt-get update && sudo apt-get install -y meson ninja-build
```

```
# some pkg needs to be installed
libsystemd-dev 
```

```sh
sudo mkdir -p /dev/hugepages
sudo mount -t hugetlbfs none /dev/hugepages
echo 1024 | sudo tee /sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages

sudo chmod 666 /dev/vfio/*

sudo ${dpdk_BINARY_DIR}/examples/dpdk-helloworld


sudo dpdk-testpmd -a 4cf0:00:02.0 -- --rxq=2 --txq=2 --forward-mode=rxonly --rss-ip -i
```

export LD_LIBRARY_PATH=/home/azureuser/code/bypass-misc/build/_deps/dpdk-build/lib:/home/azureuser/code/bypass-misc/build/_deps/dpdk-build/drivers:$LD_LIBRARY_PATH && cargo test

```sh
# Find your NIC's PCI address first
# this might not work on vm.
lspci | grep -i eth 

# Run testpmd with only that port
pci_addr=""
sudo build/dpdk-build/app/dpdk-testpmd -a "${pci_addr}" -- -i --rxq=2 --txq=2 --nb-cores=1 --rss-all
```