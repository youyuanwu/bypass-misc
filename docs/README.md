Deploy vm
```sh
rg=myrg
# create rg
az group create --name $rg --location westus2

# deploy the vm
az deployment group create \
  --resource-group $rg \
  --template-file docs/vm.bicep \
  --parameters sshPublicKey=@~/.ssh/id_rsa.pub

az group delete --name $rg
```