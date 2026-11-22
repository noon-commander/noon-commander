# 0097. The mode and the owner set apart what is ours

- Status: Accepted
- Date: 2026-11-22

## Context

The status line shows the mode and the owner of the entry under the cursor
([ADR 0096](0096-layouts-of-a-panel.md)), `-rw-r--r-- │ 501:20`. What the user wants to know of
them is what they allow the user: which of the three sets of permissions is theirs, and whether the
file or its group is theirs. Reading that off the numbers takes knowing one's uid and groups, on
this machine and on each host, where they differ. A local panel can ask the system; on a host,
`ssh -G` gives only the name of the user, and the numbers of the local machine say nothing of the
host's.

## Decision

- **The set of permissions that applies is set apart**, as the system checks them: the owner's if
  the uid is ours, else the group's if the gid is one of our groups, else the others'. It stays
  as it is; the type and the other sets are toned down, as icons are. uid 0 has no set, as none
  limits it.
- **The uid and the gid that are ours stay as they are**, the rest of the owner toned down.
- **Dim alone, no color nor bold**, so that it shows in every theme, with or without truecolor,
  and needs no new role in the palettes; what is ours in bold besides drew the eye more than the
  name.
- **Local panels go by the process**: its effective uid, its effective gid, and its supplementary
  groups (`geteuid`, `getegid`, `getgroups`).
- **A host is asked once it connects**: `id -u` and `id -G` through `Session::spawn_in`, as `rg`
  and `fd` run there ([ADR 0028](0028-programs-on-hosts-without-a-terminal.md)), both at once over
  the master. Only over the master: without one, each command would connect again, and may ask
  for a password again. What they print is kept with the connection, and goes with it.
- **Nothing is set apart while it is not known who reads the directory**: before `id` answers, on
  a host without a master, or where it answers no numbers.

## Consequences

- The mode and the owner tell at once what is ours, in the columns of a layout that shows them too.
- A connection runs two commands on the host more, once; a host without `id` sets nothing apart.
- The row under the cursor takes its colors whole, as for the units of sizes, so its columns set
  nothing apart; its status line does.
- Access control lists, capabilities, and the flags of macOS are not read: a set that applies may
  still not allow what it says, or another may.
