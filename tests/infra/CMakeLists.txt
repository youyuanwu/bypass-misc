# Azure VM deployment targets
# Requires: Azure CLI (az) logged in

# Configuration - set via cmake -D options or environment
set(AZURE_RG "tenant-test" CACHE STRING "Azure resource group name")
set(AZURE_LOCATION "westus2" CACHE STRING "Azure region")
set(SSH_PUBLIC_KEY_FILE "$ENV{HOME}/.ssh/id_rsa.pub" CACHE FILEPATH "SSH public key file")
set(VM_COUNT "1" CACHE STRING "Number of VMs to deploy (1 or 2)")
set(NICS_PER_VM "1" CACHE STRING "NICs per VM (1 or 2, use 2 for DPDK)")
set(VM_SIZE "Standard_D2s_v5" CACHE STRING "Azure VM size")

# Paths
set(BICEP_DIR ${CMAKE_CURRENT_SOURCE_DIR})
set(OUTPUTS_FILE ${CMAKE_CURRENT_BINARY_DIR}/azure-deployment-outputs.json)

# Create resource group
add_custom_target(azure_rg_create
  COMMAND az group create --name ${AZURE_RG} --location ${AZURE_LOCATION} --output table
  COMMENT "Creating Azure resource group: ${AZURE_RG}"
  VERBATIM
)

# Deploy single VM
add_custom_target(azure_vm_deploy
  COMMAND az deployment group create
    --resource-group ${AZURE_RG}
    --template-file ${BICEP_DIR}/main.bicep
    --parameters sshPublicKey="@${SSH_PUBLIC_KEY_FILE}"
                 vmCount=${VM_COUNT}
                 nicsPerVm=${NICS_PER_VM}
                 vmSize=${VM_SIZE}
    --query properties.outputs
    --output json > ${OUTPUTS_FILE}
  COMMAND ${CMAKE_COMMAND} -E echo "Deployment outputs saved to: ${OUTPUTS_FILE}"
  COMMAND cat ${OUTPUTS_FILE}
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  COMMENT "Deploying ${VM_COUNT} VM(s) to Azure resource group: ${AZURE_RG}"
  VERBATIM
)

# Get deployment outputs (after deployment) - refreshes from Azure
add_custom_target(azure_vm_save_outputs
  COMMAND az deployment group show
    --resource-group ${AZURE_RG}
    --name main
    --query properties.outputs
    --output json > ${OUTPUTS_FILE}
  COMMAND ${CMAKE_COMMAND} -E echo "=== Deployment Outputs ==="
  COMMAND cat ${OUTPUTS_FILE}
  COMMENT "Fetching deployment outputs for: ${AZURE_RG}"
  VERBATIM
)

# Get SSH command (reads from cached outputs file)
add_custom_target(azure_vm_ssh_command
  COMMAND bash -c "jq -r '.sshCommand.value' ${OUTPUTS_FILE}"
  COMMENT "Getting SSH command from ${OUTPUTS_FILE}"
  VERBATIM
)

# SSH to VM1 (reads from cached outputs file)
add_custom_target(azure_vm_ssh
  COMMAND bash -c "ssh azureuser@$$(jq -r '.vm1PublicIp.value' ${OUTPUTS_FILE})"
  COMMENT "SSH to VM1 using cached outputs"
  VERBATIM
)

# SSH to VM2 from local (reads from cached outputs file)
add_custom_target(azure_vm2_ssh
  COMMAND bash -c "ssh azureuser@$$(jq -r '.vm2PublicIp.value' ${OUTPUTS_FILE})"
  COMMENT "SSH to VM2 using cached outputs"
  VERBATIM
)

# Show cached outputs
add_custom_target(azure_vm_show_outputs
  COMMAND ${CMAKE_COMMAND} -E echo "=== Cached Deployment Outputs ==="
  COMMAND ${CMAKE_COMMAND} -E echo "File: ${OUTPUTS_FILE}"
  COMMAND cat ${OUTPUTS_FILE}
  COMMAND ${CMAKE_COMMAND} -E echo ""
  COMMAND ${CMAKE_COMMAND} -E echo "=== Quick Reference ==="
  COMMAND bash -c "echo \"VM1 Public IP: $$(jq -r '.vm1PublicIp.value' ${OUTPUTS_FILE})\""
  COMMAND bash -c "echo \"VM1 Private IP: $$(jq -r '.vm1PrivateIp.value' ${OUTPUTS_FILE})\""
  COMMAND bash -c "echo \"VM2 Public IP: $$(jq -r '.vm2PublicIp.value // \"N/A\"' ${OUTPUTS_FILE})\""
  COMMAND bash -c "echo \"VM2 Private IP: $$(jq -r '.vm2PrivateIp.value // \"N/A\"' ${OUTPUTS_FILE})\""
  COMMAND bash -c "echo \"SSH Command: $$(jq -r '.sshCommand.value' ${OUTPUTS_FILE})\""
  COMMENT "Displaying cached deployment outputs"
  VERBATIM
)

# Delete resource group (cleanup)
add_custom_target(azure_rg_delete
  COMMAND az group delete --name ${AZURE_RG} --yes --no-wait
  COMMENT "Deleting Azure resource group: ${AZURE_RG} (async)"
  VERBATIM
)

# Show current status
add_custom_target(azure_vm_status
  COMMAND ${CMAKE_COMMAND} -E echo "=== Resource Group: ${AZURE_RG} ==="
  COMMAND az vm list --resource-group ${AZURE_RG} --output table || true
  COMMAND ${CMAKE_COMMAND} -E echo ""
  COMMAND ${CMAKE_COMMAND} -E echo "=== Public IPs ==="
  COMMAND az network public-ip list --resource-group ${AZURE_RG} --output table || true
  COMMENT "Showing VM status in: ${AZURE_RG}"
  VERBATIM
)

# Lint bicep files
add_custom_target(azure_bicep_lint
  COMMAND az bicep lint --file ${BICEP_DIR}/main.bicep
  COMMAND az bicep lint --file ${BICEP_DIR}/modules/vm.bicep
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  COMMENT "Linting Bicep templates"
  VERBATIM
)
