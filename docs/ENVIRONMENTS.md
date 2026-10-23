# Environments

A named set of variables and files a team gives the sandboxes it creates.

```sh
curl -sS -X PUT $NODE/environments/web -H "x-api-key: $KEY" -H 'content-type: application/json' -d '{
  "envVars": {"API_URL": "https://api.example", "NPM_TOKEN": "..."},
  "files": [
    {"path": "/home/user/.npmrc", "content": "//registry.npmjs.org/:_authToken=${NPM_TOKEN}\n"},
    {"path": "/etc/app/cert.der", "contentBase64": "MIIB..."}
  ]}'

curl -sS -X POST $NODE/sandboxes -H "x-api-key: $KEY" \
  -d '{"templateID":"base","environment":"web","envVars":{"MODE":"debug"}}'

curl -sS $NODE/environments -H "x-api-key: $KEY"        # names and sizes only
curl -sS -X DELETE $NODE/environments/web -H "x-api-key: $KEY"
```

| Route | |
|---|---|
| `PUT /environments/{name}` | Make it, or replace it whole. `envVars`, and `files` each with `path` and either `content` (text) or `contentBase64` |
| `GET /environments`, `GET /environments/{name}` | The caller's team's: variable names, file paths and sizes. Never a value or a file's contents |
| `DELETE /environments/{name}` | Remove it |

- **At creation**, a sandbox given `"environment": NAME` has its variables in
  every command, under its own `envVars`, which win where both name one, and
  its files written into the guest, owned by the template's user, before
  the create answers.
- **They stay with the sandbox**: a reboot puts them back and a fork has
  them. A change to the environment applies to sandboxes created after it;
  one already running keeps what it started with.
- **Per team.** A team sees, uses and replaces only its own; two teams may
  each have one of the same name.
- **Limits:** 1,024 variables and 128 KiB of them; 64 files and 4 MiB of
  them; absolute paths with no `..`; 256 environments a team.
- **A node and a control plane both serve them**, from the cluster store, so
  every node starts a sandbox with the same environment.

## Repositories

An environment can hold [Lit](https://github.com/nervosys/Lit) repositories,
nervosys' version control system. The node clones each one when a sandbox is
created with the environment, and puts the tree, `.lit/` and all, in its
place in the guest before the create answers.

```json
{"repositories": [
  {"remote": "https://lit.example/team/app", "path": "/srv/app",
   "branch": "main", "username": "ci", "password": "..."}
]}
```

| Field | |
|---|---|
| `kind` | `lit` (the default) or `git`: which program clones it. A Git repository takes no `username` or `password`; a private one is reached with [the node's credential](#private-git-remotes) |
| `remote` | A `lit server`'s URL, `http`, `https` or `lit`, with no credentials in it; or, for `git`, an `http` or `https` Git remote |
| `path` | Where in the guest; made if it is not there, and the tree given to the user commands run as |
| `branch` | Checked out after the clone; the remote's default unless given |
| `username`, `password` | An account on the remote, logged in to for each clone; both or neither. The password is kept as the rest of an environment is and never returned |

- **The node clones, not the guest.** The guest needs no network and no
  `lit`. The node runs the `lit` binary its operator names with `--lit`, as a
  program: HyperMachine links no Lit code.
- **Only from remotes the operator allows**, with `--lit-remote URL`
  (repeatable): the same scheme, host and port, and a path at or below it.
  Anything else is refused with 403, so an environment cannot make the node
  fetch whatever it names.
- **It is cloned when a sandbox is created**, so a new sandbox has the
  remote as it is then. A reboot puts back that same tree; a fork has it.
- **Refused before anything boots**: a remote not allowed, a login refused,
  a branch the remote does not have. A sandbox is not left behind.
- **At most 8 an environment, 64 MiB each** once packed.

### Private Git remotes

An environment never holds a Git password, so the store holds none. The
operator gives the *node* a credential instead:

```
export GIT_TOKEN='ci-bot:ghp_...'          # user:token
hv2-sandboxd --git-remote https://git.example/team/ \
             --git-credential https://git.example/team/=GIT_TOKEN
```

- The value is read from the node's own environment variable at start-up
  (the flag names the variable, so the token is not in `ps` or a unit file's
  `ExecStart`). A node that cannot read it, or finds no `user:token`, does
  not start.
- It is sent, as HTTP Basic, only to remotes under the flag's prefix (same
  scheme, host and port, and a path at or below it). Redirects are not
  followed, so it cannot be led off that prefix. The first matching
  `--git-credential` wins; the remote must also be allowed by `--git-remote`.
- It reaches `git` in the process's environment, not its arguments, so it is
  not written into the clone's `.git/config`, and it never enters the guest.
  A failed clone's message has it removed.
- For GitHub, use a fine-grained token with read access to the repository
  and any user name; for GitLab, `oauth2` and a token.

[Checked on KVM](benchmarks/2026-10-17/git-private-kvm/README.md) against
a local server that wants the credential.

#### From inside a sandbox

The same credential can serve a sandbox's *own* `git` (a `git clone`, `fetch`
or `push` an agent runs), with the credential still only on the host:

```
hv2-sandboxd --network --git-credential https://git.example/team/=GIT_TOKEN \
             --git-credential-in-sandbox
```

- The sandbox's gateway, which already ends its TLS, sets
  `Authorization: Basic ...` on a request whose **scheme, host, port and path
  prefix** are the credential's URL, and **replaces** any `Authorization`
  the guest wrote. The guest holds no token, none is in its environment or
  `.git/config`, and `git` asks for none. A request elsewhere on the host, a
  path that climbs out with `..` or an encoded dot or slash, a look-alike
  host or another port, goes as the guest wrote it.
- **Opt-in for the whole node.** Every sandbox on a node started with the
  flag, whose egress policy lets it reach the host, can use the credential
  for requests under its prefix, whoever's it is. A credential grants no
  network access: the sandbox's own `allowOut` and `denyOut` still decide
  whether it can reach the host. Give a node that runs for several teams a
  token for something they may all read, or do not set the flag.
- Only `https` prefixes (the gateway sets it inside TLS), and the node must
  have `--network`. The gateway verifies the server's certificate against the
  public roots and `--egress-upstream-ca`, as for any intercepted host. The
  guest has to trust the gateway's CA, which the node installs at boot in
  `/etc/ssl/certs/ca-certificates.crt` (what Debian's `git` and `curl`
  read).
- The server sees the token, as it would from the node's own clone, and
  **so does any program in the sandbox that can make the server echo its
  request headers**, if the server has such an endpoint under the prefix.
  The prefix is the limit, so scope it to what the token is for.

Checked on [real KVM](benchmarks/2026-10-22/git-in-sandbox-kvm/README.md)
with the host's `git` 2.47 and `curl` in the guest, against an owned HTTPS
server.

What [Lit 1.8.0](benchmarks/2026-10-12/lit-repositories-kvm/README.md)
needs today: the account must be an **admin**, since its server lets no other
role clone; and a session token expires, so the node logs in for each clone
with the stored password rather than keeping a token. Both are with Lit's
maintainers.

**Taken together, that means an environment holding a repository holds an
admin password for its Lit server**, in the cluster store, unencrypted, and
every node that clones for it uses that password. Whoever can read the store
can administer that Lit server. Until Lit lets a read-only account clone, or
offers a read-scoped token, give each environment's repositories a Lit server
of their own rather than one that matters for anything else.

## What is kept, and where

An environment is kept in the cluster store as given, values and contents
included, **unencrypted**: a secret put in one is as safe as the store. A
lone node keeps them in memory and forgets them when it restarts. They are
not returned by the API and not written to the node's log.

## Not here

- **Setup commands** run at creation; a [template](../README.md) runs its
  steps when it is built.
- **Machines.** A machine's guest takes `userData` for cloud-init.
- **The `hm` command line.** The routes are reached with any HTTP client.

[Checked on real KVM](benchmarks/2026-10-12/environments-kvm/README.md).
