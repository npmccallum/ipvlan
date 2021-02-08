# Welcome to ipvlan

This utility aims to create `ipvlan` network namespaces for regular
(unprivileged) users. This allows a process, container or even a VM to have an
`ipvlan` interface.

## What is an ipvlan interface?

An `ipvlan` interface is a type of virtual networking interface created by
Mahesh Bandewar for the Linux kernel. It is conceptually similar to a
`macvlan`, but works at layer 3. For more information, see the [Linux kernel
documentation](https://www.kernel.org/doc/html/latest/networking/ipvlan.html).

## How does the ipvlan utility work?

Using `ipvlan` is simple! Just prefix `ipvlan` to your executable invocation
and you'll get your very own `ipvlan` configuration.

```
$ cat /etc/ipvlan.conf
12.34.56.0/24
$ ipvlan ip addr show dev ipvl0
5: ipvl0@if2: <BROADCAST,NOARP,UP,LOWER_UP> mtu 1500 qdisc noqueue state UNKNOWN group default qlen 1000
    link/ether d4:d2:52:40:82:d8 brd ff:ff:ff:ff:ff:ff link-netnsid 0
    inet 12.34.56.78/24 scope global ipvl0
       valid_lft forever preferred_lft forever
    inet6 fe80::d4d2:5200:140:82d8/64 scope link
       valid_lft forever preferred_lft forever
$ ip addr show dev ipvl0
Device "ipvl0" does not exist.
```

## How do I install ipvlan?

TODO

## Is ipvlan secure?

We hope to have made `ipvlan` reasonably secure. If there is a problem, please
let us know! Let's go over the security properties of `ipvlan`.

#### The Configuration File

The `ipvlan` configuration file is central to the security of `ipvlan`. In it,
the system administrator defines subnets from which ipvlan instances can be
created. During initialization, `ipvlan` checks the permissions on the
configuration file to ensure that misconfiguration hasn't occurred. A
configuration file will only be used under the following conditions:

1. The configuration file **MUST** be owned as root.
2. The configuration file **MUST** not be writable by anyone other than the owner.
3. The configuration file **MUST** be on the same filesystem as the `ipvlan` binary.

So long as the above conditions are true, `ipvlan` can be used by anyone who
can read the configuration file. This means that the system administrator can
control who is allowed to allocation ipvlan instances by controlling who can
read the configuration file.

#### The Application Executable

The `ipvlan` executable is Linux capability-aware. It requires three
capabilities in the **permitted** set:

* `CAP_DAC_OVERRIDE`
* `CAP_SYS_ADMIN`
* `CAP_NET_ADMIN`

You can set using this simple command (after install):

```
$ sudo setcap "cap_dac_override,cap_sys_admin,cap_net_admin+p" /usr/bin/ipvlan
```

We take care only to enable these capabilities when needed and to drop them
from the **permitted** set as soon as they are no longer needed.

The `ipvlan` executable does the following:

1. First, it finds the gateway interface and address for each subnet.
2. Then it chooses an address from each subnet to assign to the ipvlan interface.
3. Next it validates that the address isn't currently in use by any namespace.
   Afer this validation, `CAP_DAC_OVERRIDE` is dropped from **permitted**.
4. One we have successfully identified valid addresses to use, we create
   the new namespace and its ipvlan interface(s). `CAP_SYS_ADMIN` is dropped
   from **permitted**.
5. Next the addresses are assigned to the interfaces, they are brought up and
   routes are created. `CAP_SYS_ADMIN` is dropped from **permitted**.
6. Finally, the next executable is executed.

Since this process may be subject to race conditions, `ipvlan` exclusively locks
the configuration file during execution to ensure that only one instance executes
at the same time.

So long as the next executable executed does not itself have elevated privileges
(i.e setuid root or filesystem capabilities), it will not be able to modify the
ipvlan interface or the namespace it resides in. Therefore, no elevated
permissions are given to the subsequent binary. Since `ipvlan` does not continue
to run, but fully transitions to the new executable (i.e. `execve()`), once the
namespace is no longer in use the interface is automatically destroyed and its
addresses are recycled for future use.

#### Advice to sysadmins

1. Be careful with the permissions on the configuration file.
2. Besides setting up the gateway address/interface manually, let `ipvlan` manage
   the subnet. Don't manually create interfaces using these subnet IPs. Bad things
   will happen. You have been warned!
