Deploy vm
```sh
# create rg
az group create --name $rg --location westus2

# deploy the vm
az deployment group create \
  --resource-group $rg \
  --template-file docs/vm.bicep \
  --parameters sshPublicKey="$(cat ~/.ssh/id_rsa.pub)"
```