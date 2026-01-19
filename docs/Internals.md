# Internals

```
ARP storm: Constant address 10.0.0.1 not in neighbor cache, sending ARP request messages
Each queue has its own smoltcp interface with its own ARP cache - they're all independently trying to ARP for the gateway!

Connections fail: connection closed before message completed because the SYN-ACK can't be sent (no ARP response)
The root cause: ARP replies go to only ONE queue (probably queue 0), but all 8 queues are sending ARP requests. Queues 1-7 never receive the ARP reply, so they can never populate their neighbor cache.
```

We inject a ARP packet to smoltcp at startup for GW address.