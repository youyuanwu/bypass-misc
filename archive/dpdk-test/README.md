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
```

export LD_LIBRARY_PATH=/home/azureuser/code/bypass-misc/build/_deps/dpdk-build/lib:/home/azureuser/code/bypass-misc/build/_deps/dpdk-build/drivers:$LD_LIBRARY_PATH && cargo test