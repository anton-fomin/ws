# Security

Please report a vulnerability privately, through
[GitHub's private vulnerability reporting](https://github.com/anton-fomin/ws/security/advisories/new),
not in a public issue.

Only the latest release is supported. `ws` is maintained by one person: expect
an answer within a week, and a fix in the next release.

`ws` runs the `setup` commands of the `ws.yaml` you give it, through `sh`. That
is what the file is for, so a `ws.yaml` from a source you do not trust is code
you do not trust.
