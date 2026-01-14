param virtualMachines_test1_name string = 'test1'
param publicIPAddresses_test1_ip_name string = 'test1-ip'
param publicIPAddresses_test1_ip2_name string = 'test1-ip2'
param virtualNetworks_test1_vnet_name string = 'test1-vnet'
param networkInterfaces_test1809_z1_name string = 'test1809_z1'
param networkInterfaces_test1809_z2_name string = 'test1809_z2'
param networkSecurityGroups_test1_nsg_name string = 'test1-nsg'
param schedules_shutdown_computevm_test1_name string = 'shutdown-computevm-test1'
@description('SSH public key for VM access')
param sshPublicKey string

resource networkSecurityGroups_test1_nsg_name_resource 'Microsoft.Network/networkSecurityGroups@2024-07-01' = {
  name: networkSecurityGroups_test1_nsg_name
  location: 'westus2'
  properties: {
    securityRules: []
  }
}

resource publicIPAddresses_test1_ip_name_resource 'Microsoft.Network/publicIPAddresses@2024-07-01' = {
  name: publicIPAddresses_test1_ip_name
  location: 'westus2'
  sku: {
    name: 'Standard'
    tier: 'Regional'
  }
  zones: [
    '1'
  ]
  properties: {
    ipAddress: '20.115.209.127'
    publicIPAddressVersion: 'IPv4'
    publicIPAllocationMethod: 'Static'
    idleTimeoutInMinutes: 4
    ipTags: []
    ddosSettings: {
      protectionMode: 'VirtualNetworkInherited'
    }
  }
}

resource publicIPAddresses_test1_ip2_name_resource 'Microsoft.Network/publicIPAddresses@2024-07-01' = {
  name: publicIPAddresses_test1_ip2_name
  location: 'westus2'
  sku: {
    name: 'Standard'
    tier: 'Regional'
  }
  zones: [
    '1'
  ]
  properties: {
    publicIPAddressVersion: 'IPv4'
    publicIPAllocationMethod: 'Static'
    idleTimeoutInMinutes: 4
    ipTags: []
    ddosSettings: {
      protectionMode: 'VirtualNetworkInherited'
    }
  }
}

resource virtualNetworks_test1_vnet_name_resource 'Microsoft.Network/virtualNetworks@2024-07-01' = {
  name: virtualNetworks_test1_vnet_name
  location: 'westus2'
  properties: {
    addressSpace: {
      addressPrefixes: [
        '10.0.0.0/16'
      ]
    }
    privateEndpointVNetPolicies: 'Disabled'
    subnets: [
      {
        name: 'default'
        properties: {
          addressPrefix: '10.0.0.0/24'
          delegations: []
          privateEndpointNetworkPolicies: 'Disabled'
          privateLinkServiceNetworkPolicies: 'Enabled'
        }
        type: 'Microsoft.Network/virtualNetworks/subnets'
      }
    ]
    virtualNetworkPeerings: []
    enableDdosProtection: false
  }
}

resource virtualMachines_test1_name_resource 'Microsoft.Compute/virtualMachines@2024-11-01' = {
  name: virtualMachines_test1_name
  location: 'westus2'
  zones: [
    '1'
  ]
  properties: {
    hardwareProfile: {
      vmSize: 'Standard_D2s_v5'
    }
    additionalCapabilities: {
      hibernationEnabled: false
    }
    storageProfile: {
      imageReference: {
        publisher: 'canonical'
        offer: 'ubuntu-24_04-lts'
        sku: 'server'
        version: 'latest'
      }
      osDisk: {
        osType: 'Linux'
        name: '${virtualMachines_test1_name}_OsDisk_1_f76f27a75d144b5ca2bee2c7320aa825'
        createOption: 'FromImage'
        caching: 'ReadWrite'
        managedDisk: {
          storageAccountType: 'Premium_LRS'
          id: resourceId(
            'Microsoft.Compute/disks',
            '${virtualMachines_test1_name}_OsDisk_1_f76f27a75d144b5ca2bee2c7320aa825'
          )
        }
        deleteOption: 'Delete'
        diskSizeGB: 30
      }
      dataDisks: []
      diskControllerType: 'SCSI'
    }
    osProfile: {
      computerName: virtualMachines_test1_name
      #disable-next-line adminusername-should-not-be-literal
      adminUsername: 'azureuser'
      linuxConfiguration: {
        disablePasswordAuthentication: true
        ssh: {
          publicKeys: [
            {
              path: '/home/azureuser/.ssh/authorized_keys'
              keyData: sshPublicKey
            }
          ]
        }
        provisionVMAgent: true
        patchSettings: {
          patchMode: 'ImageDefault'
          assessmentMode: 'ImageDefault'
        }
      }
      secrets: []
      allowExtensionOperations: true
      requireGuestProvisionSignal: true
    }
    securityProfile: {
      uefiSettings: {
        secureBootEnabled: true
        vTpmEnabled: true
      }
      securityType: 'TrustedLaunch'
    }
    networkProfile: {
      // Use 2 NIC so that 1 NIC can be used by DPDK
      networkInterfaces: [
        {
          id: networkInterfaces_test1809_z1_name_resource.id
          properties: {
            deleteOption: 'Detach'
            primary: true
          }
        }
        {
          id: networkInterfaces_test1809_z2_name_resource.id
          properties: {
            deleteOption: 'Detach'
            primary: false
          }
        }
      ]
    }
    diagnosticsProfile: {
      bootDiagnostics: {
        enabled: true
      }
    }
  }
}

resource schedules_shutdown_computevm_test1_name_resource 'microsoft.devtestlab/schedules@2018-09-15' = {
  name: schedules_shutdown_computevm_test1_name
  location: 'westus2'
  properties: {
    status: 'Enabled'
    taskType: 'ComputeVmShutdownTask'
    dailyRecurrence: {
      time: '1900'
    }
    timeZoneId: 'UTC'
    notificationSettings: {
      status: 'Disabled'
      timeInMinutes: 30
      notificationLocale: 'en'
    }
    targetResourceId: virtualMachines_test1_name_resource.id
  }
}

resource networkSecurityGroups_test1_nsg_name_SSH 'Microsoft.Network/networkSecurityGroups/securityRules@2024-07-01' = {
  parent: networkSecurityGroups_test1_nsg_name_resource
  name: 'SSH'
  properties: {
    protocol: 'TCP'
    sourcePortRange: '*'
    destinationPortRange: '22'
    sourceAddressPrefix: '*'
    destinationAddressPrefix: '*'
    access: 'Allow'
    priority: 300
    direction: 'Inbound'
    sourcePortRanges: []
    destinationPortRanges: []
    sourceAddressPrefixes: []
    destinationAddressPrefixes: []
  }
}

resource networkSecurityGroups_test1_nsg_name_DPDK 'Microsoft.Network/networkSecurityGroups/securityRules@2024-07-01' = {
  parent: networkSecurityGroups_test1_nsg_name_resource
  name: 'DPDK_TCP_Server'
  properties: {
    protocol: 'TCP'
    sourcePortRange: '*'
    destinationPortRange: '8080'
    sourceAddressPrefix: '*'
    destinationAddressPrefix: '*'
    access: 'Allow'
    priority: 310
    direction: 'Inbound'
    sourcePortRanges: []
    destinationPortRanges: []
    sourceAddressPrefixes: []
    destinationAddressPrefixes: []
  }
}

resource virtualNetworks_test1_vnet_name_default 'Microsoft.Network/virtualNetworks/subnets@2024-07-01' = {
  parent: virtualNetworks_test1_vnet_name_resource
  name: 'default'
  properties: {
    addressPrefix: '10.0.0.0/24'
    delegations: []
    privateEndpointNetworkPolicies: 'Disabled'
    privateLinkServiceNetworkPolicies: 'Enabled'
  }
}

resource networkInterfaces_test1809_z1_name_resource 'Microsoft.Network/networkInterfaces@2024-07-01' = {
  name: networkInterfaces_test1809_z1_name
  location: 'westus2'
  // kind: 'Regular'
  properties: {
    ipConfigurations: [
      {
        name: 'ipconfig1'
        type: 'Microsoft.Network/networkInterfaces/ipConfigurations'
        properties: {
          privateIPAddress: '10.0.0.4'
          privateIPAllocationMethod: 'Dynamic'
          publicIPAddress: {
            id: publicIPAddresses_test1_ip_name_resource.id
            properties: {
              deleteOption: 'Detach'
            }
          }
          subnet: {
            id: virtualNetworks_test1_vnet_name_default.id
          }
          primary: true
          privateIPAddressVersion: 'IPv4'
        }
      }
    ]
    dnsSettings: {
      dnsServers: []
    }
    enableAcceleratedNetworking: true
    enableIPForwarding: false
    disableTcpStateTracking: false
    networkSecurityGroup: {
      id: networkSecurityGroups_test1_nsg_name_resource.id
    }
    nicType: 'Standard'
    auxiliaryMode: 'None'
    auxiliarySku: 'None'
  }
}

resource networkInterfaces_test1809_z2_name_resource 'Microsoft.Network/networkInterfaces@2024-07-01' = {
  name: networkInterfaces_test1809_z2_name
  location: 'westus2'
  // kind: 'Regular'
  properties: {
    ipConfigurations: [
      {
        name: 'ipconfig1'
        type: 'Microsoft.Network/networkInterfaces/ipConfigurations'
        properties: {
          privateIPAddress: '10.0.0.5'
          privateIPAllocationMethod: 'Dynamic'
          publicIPAddress: {
            id: publicIPAddresses_test1_ip2_name_resource.id
            properties: {
              deleteOption: 'Detach'
            }
          }
          subnet: {
            id: virtualNetworks_test1_vnet_name_default.id
          }
          primary: true
          privateIPAddressVersion: 'IPv4'
        }
      }
    ]
    dnsSettings: {
      dnsServers: []
    }
    enableAcceleratedNetworking: true
    enableIPForwarding: false
    disableTcpStateTracking: false
    networkSecurityGroup: {
      id: networkSecurityGroups_test1_nsg_name_resource.id
    }
    nicType: 'Standard'
    auxiliaryMode: 'None'
    auxiliarySku: 'None'
  }
}
