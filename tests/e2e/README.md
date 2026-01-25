# E2E Tests with Ansible

End-to-end tests that run against Azure VMs deployed with the Bicep templates.

## Prerequisites

```sh
pip install ansible
```

## Setup

1. Deploy VMs:
   ```sh
   cd build && make azure_vm_deploy
   ```

2. Verify inventory:
   ```sh
   ./tests/e2e/inventory.py --list
   ```

## Run Tests

```sh
# Hello world (basic connectivity)
./tests/e2e/run_tests.sh

# Or run specific playbook
./tests/e2e/run_tests.sh playbooks/hello_world.yml
./tests/e2e/run_tests.sh playbooks/test_connectivity.yml

# HTTP server tests with different backends
./tests/e2e/run_tests.sh playbooks/http_server_test.yml                         # DPDK mode (default)
./tests/e2e/run_tests.sh playbooks/http_server_test.yml -e server_mode=tokio       # Tokio multi-threaded
./tests/e2e/run_tests.sh playbooks/http_server_test.yml -e server_mode=tokio-local # Tokio thread-per-core

# Direct ansible-playbook usage (from tests/e2e directory)
cd tests/e2e && ansible-playbook playbooks/hello_world.yml -v


# Run full test for all modes
./tests/e2e/run_all_modes.sh

# Generate md.
python3 tests/e2e/generate_benchmark_report.py
cp build/benchmarks/BENCHMARK_COMPARISON.md docs/Bench/Bench-Temp.md
```

## Playbooks

| Playbook | Description |
|----------|-------------|
| `hello_world.yml` | Basic connectivity, OS info |
| `test_connectivity.yml` | Private network ping between VMs |

## Dynamic Inventory

The `inventory.py` script reads VM IPs from:
```
build/docs/azure-deployment-outputs.json
```

This file is created by `make azure_vm_deploy` or `make azure_vm_outputs`.

## Adding New Tests

Create a new playbook in `playbooks/` directory:

```yaml
---
- name: My Test
  hosts: vms
  tasks:
    - name: Run something
      ansible.builtin.shell: echo "test"
```
