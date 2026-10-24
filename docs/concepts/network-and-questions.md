---
title: Network and questions
sidebar_label: Network and questions
description: How every connection a task makes goes through its proxy, which hosts pass, how questions wait for your answer, and how to open or shut the network.
---

# Network and questions

## Every connection goes through the task's proxy

Each task has its own **proxy**. Commands in the task can connect only to
it: the usual proxy variables (`HTTPS_PROXY` and the like) point at it, and
the sandbox keeps everything else out of reach.

The proxy judges each connection with the same [policy](policy.md) as
files (the effect kind `net.connect`, with a host and a port):

| Connection | Verdict (balanced and careful) |
| --- | --- |
| Package registries over HTTPS: npm, Yarn, crates.io, NuGet, PyPI, RubyGems, Maven Central | **pass** |
| The agent's own model API, while that agent runs | **pass** |
| Anything else, git hosts included | **ask** |

Git hosts ask because, inside TLS, a push cannot be told apart from a fetch.

Every connection lands in the task's `network.jsonl` and is printed after a
command:

```text
net: registry.npmjs.org:443 · tunneled (package-registries)
net: example.com:443 · refused, would wait for approval; the command line asks no one (no-rule-lets-it-pass)
net: cdn.example:443 · cut by Kule: its TLS hello asks for other.example, and this tunnel is to cdn.example (agent-api)
```

A connection that did not go through as asked says why on its line: cut by
Kule, refused by Kule for the address it led to, failed on its own, or did
not resolve.

Each connection's outcome says what became of it:

| Outcome | What became of it |
| --- | --- |
| `tunneled` | It went through. Where Kule looks inside it, every request in it went through too. |
| `refused` | The policy did not let it through, nobody approved it, or Kule refused it for the address its name led to (the log's `refused` field says which). |
| `cut` | Kule let it through, then ended it, and says why in the log's `cut` field: see [HTTPS and secrets](#https-and-secrets). |
| `failed` | It ended in an error of its own, not Kule's doing: the host went away, the connection was reset. The log's `failed` field says why. This holds for plain HTTP and for connections Kule looks inside alike. |
| `unresolved`, `private`, `unreachable` | Its name did not resolve, every address it led to is private or this computer's, or the host did not take the connection. |

The proxy also checks the address it connects to, and keeps private
addresses out of reach. For IPv6 only the global unicast range counts as
public; an IPv4 address carried inside an IPv6 one (mapped, NAT64, 6to4)
is judged as itself. A name is judged in lower case and without a final
dot, so `Example.com.` is `example.com`.

## Questions

A connection whose verdict is **ask** becomes a **question**: it waits for
you.

**In the app**, a card shows on the task's Live panel and the task moves
under **Needs you**. The card says the host and port, the rule that made it
wait, and a countdown. While the window is not in front, a system
notification tells you.

The card also says what approving lets through:

- **Tunneled, not looked into.** The usual case. Kule sees the host and
  port, not what goes over the connection, and checks that its TLS hello
  asks for this host. Approving comes before the hello, so the card can
  only state the rule: if the hello asks for another host or hides its
  name, Kule cuts the connection after your approval.
- **Looked into.** The host is one Kule fills a secret for (the card names
  the secrets, as in "fills STRIPE_KEY into its requests") or a package
  registry it holds to reads. An approval never adds a host to the ones
  Kule looks into.

Once answered, the card becomes a line in the run, where it was asked:
"You approved api.example:443". If Kule then cuts the connection, or refuses
it because its name led to an address on this computer or its network, the
same line says so, with why, while the run is live, and a notification
tells you: "You approved api.example:443; then cut by Kule: …".

| Answer | Shortcut | Effect |
| --- | --- | --- |
| **Approve** | Mod+Shift+Y | This connection goes through. |
| **Approve and remember** | – | This host and port go through on this folder from now on. |
| **Deny** | Mod+Shift+N | Refused; the agent is told to go on without it. |

**Unanswered**, the connection is refused when the task's wait runs out:
15 minutes by default, settable from 1 minute to 8 hours (the **waits**
picker under the prompt). It cannot be turned off.

Kule refuses a question without your answer in three cases, and says so
as its own refusal, never as yours: nobody answered in time; the run ended
before anyone answered; or 16 of the run's questions were waiting already,
so the next one is refused without asking (told once per host and port).
Each shows as a line in the run ("Kule refused api.example:443: nobody
answered in time"), a notification, and in the Timeline.

**From the command line**, nobody is there to ask, so `kule exec` and
`kule run` refuse such connections at once (`would wait for approval; the
command line asks no one`).

Remembered hosts are listed by [`kule rules`](../cli/rules.md) and in
*Settings → Rules*, where you can forget one.

## Shutting or opening the network

For one command:

```sh
kule exec TASK --no-network -- npm test     # no TCP at all, localhost included
kule exec TASK --open-network -- ./fetch.sh # connect anywhere: no proxy, no policy
```

`--open-network` bypasses the policy entirely. Use it knowingly.

## HTTPS and secrets

Normally the proxy only tunnels HTTPS: it sees the host and port, not the
content. Before a tunnel goes on, it reads the client's TLS hello: the name
it asks for must be the host the tunnel was opened to (none for an
address), or the tunnel is cut before a byte reaches the host, as many
hosts share one address and serve whichever name a hello asks for. A
hello that hides its name (Encrypted Client Hello) is cut too: what turns
it off in each browser and tool is in
[Troubleshooting](../troubleshooting.md#a-connection-is-cut-for-encrypted-client-hello).
Port 443 carries nothing but
TLS. Plain `http://` requests are read one by one, and each must be for the
host it was let through to. A connection whose name led to an address on
this computer or its network is cut too. A cut connection is logged as
`cut`, with why.

For hosts that a [secret](secrets.md) belongs to, it looks inside
TLS to put the secret's value into the request. For that it makes a
certificate authority of its own for each run, valid only for those hosts,
and points `SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`, `REQUESTS_CA_BUNDLE` and
the like at it. Kule never adds it to your keychain.

Package registries are **read-only** to commands (a publish is refused) for
npm, yarn, pnpm and bun, cargo, pip, uv and poetry, and gem, whose tools
trust the task's authority.

Where Kule looks inside a connection and answers a request itself, the
connection is logged as `cut`, with `Kule refused a request in it: <why>`.
The why is one of these, in Kule's words, never the request's:

- `a write to a host commands may only read from`
- `a credential Kule does not hold for this host`
- `a body Kule checks whole that was chunked or over 1 MiB`
- `a request Kule could not read one way only, or for another host`

A plain `http://` request Kule refused is logged as `cut` too, with why.

## Known limits

These are listed in [Platforms](../platforms.md) too:

- HTTP/2 is not spoken: clients fall back to HTTP/1.1. WebSocket upgrades
  are not supported yet.
- On macOS, tools that check certificates only through the system's own
  service cannot use HTTPS in a task. pip 24.2 and later works with
  `--use-deprecated=legacy-certs`. Go is told to use the roots
  `SSL_CERT_FILE` names.
- NuGet's and Maven's tools on macOS trust the system's roots alone, so
  their registries pass through unseen, as Go's module proxy does.
- On Linux, network rules need Linux 6.7 or later.

## Related

- [`kule exec`](../cli/exec.md), [`kule run`](../cli/run.md), [`kule rules`](../cli/rules.md)
- [Policy](policy.md), [Secrets](secrets.md)
- [Live](../app/live.md), [Timeline](../app/timeline.md)
