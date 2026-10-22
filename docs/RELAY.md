# Host your own relay

*Version française : [fr/RELAY.md](fr/RELAY.md)*

Live sharing ([SHARING.md](SHARING.md)) needs a small **relay** so that two computers can find each other and, when they cannot reach each other directly, exchange data. By default Inkboard uses the free public relays of the iroh project (n0). You can run your own, with the official `iroh-relay` program. This page explains when it is worth it, how to set it up, and how to point Inkboard at it.

> **Status and honesty.** The example files in [`packaging/relay/`](../packaging/relay/) were checked for syntax only. A complete run on a real server with a real domain name, a certificate and real guests has **not** been tried. What is "not verified" is marked as such below. Sources: the iroh documentation (docs.iroh.computer, "Self-hosted" and "Rate limiting" pages, read on 2026-10-22), the source code of `iroh-relay` 1.4.0, and our own tests (spike S1).

## What a relay is (and is not)

- **Stateless**: it stores nothing; there is nothing to back up. It forwards bytes between computers.
- **It only sees encrypted data.** Inkboard sessions are end-to-end encrypted. A relay still sees network metadata: IP addresses, times, volumes (as the iroh documentation says of relays in general).
- **It is not a session server.** The host's computer is still the server of the session; if the host leaves, the session ends.
- Once two computers manage to connect directly, the data stops going through the relay.

## When is it useful?

- **Serious or regular use.** The public relays are, per iroh, for "development and hobby use", with no guarantee of service, rate-limited per connection (limits not published; we measured 5 to 17 Mbit/s) and only supported for the latest version. For a team that shares boards every day, your own relay removes those limits.
- **A company that allows one specific domain.** In our test on a corporate network with a cloud security gateway, the public relay's domain was refused (HTTP 403 with a warning page, "unknown category"), and sessions could not start. A company can run a relay on a domain it controls and ask its own IT department to allow it, so that its staff can share boards. This is the only fix: Inkboard does not try to bypass a network's rules.
- **Keeping the metadata (IP addresses, times) within your own infrastructure.**

If you only share occasionally between people on open networks, the public relays are enough.

## Prerequisites

- A **machine with a public IP address** (a small VPS is enough; the relay forwards data but does not compute much). Capacity under real load (20 people, large images) has not been measured.
- A **domain name** (for example `relay.example.org`) with an A and/or AAAA record pointing at the machine.
- Open **TCP 443** (the relay, in HTTPS and WebSocket) and **TCP 80** (the relay listens on it by default). Optional **UDP 7842**, only if you enable QUIC address discovery (see below).
- To use the example files: **Docker with Compose**. Without Docker, you can use the `iroh-relay` program directly: binaries on the iroh releases page, or `cargo install iroh-relay --version 1.4.1 --features server --locked` (the `server` feature is required; we built version 1.4.0 this way in our test).
- Recent Inkboard builds use `iroh` 1.4.1. Keep the relay on the same 1.x series. Per the iroh documentation, the series is 1.x, the protocol is backward compatible with the previous major version, and a minor version is fully supported for about three months.

## Install with the example kit

The kit is in `packaging/relay/`: `docker-compose.yml`, `relay.toml` and a short `README.md`.

1. Copy the `packaging/relay/` folder to your machine.
2. Edit `relay.toml`: set `hostname` to your domain and `contact` to your e-mail address (required for Let's Encrypt).
3. Start it:
   ```
   docker compose up -d
   ```
4. Check that it answers (from another machine):
   ```
   curl --noproxy '*' --fail --show-error https://relay.example.org/healthz
   ```

### Automatic TLS (Let's Encrypt)

With `cert_mode = "LetsEncrypt"`, the relay gets and renews its certificate by itself; you need the domain name to point at the machine, and port 443 reachable from the Internet (the program's source answers the "TLS-ALPN-01" challenge on 443). The certificate cache is kept in the `relay-certs` volume: it is not precious (the relay would ask for a new one), but keep it to avoid hitting Let's Encrypt's own rate limits when you restart often. Use `prod_tls = false` only for tests: that selects Let's Encrypt's staging service, whose certificates are not trusted.

TLS is **required in practice** for Inkboard's future browser version: a web page in `https://` cannot open a non-encrypted WebSocket (mixed content; not tested). The native application is not subject to that rule, but a relay without TLS (plain HTTP) is only meant for local tests.

### Configuration (`relay.toml`)

The options used by the example, and what they do (verified in the source of `iroh-relay` 1.4.0):

| Option | Meaning |
|---|---|
| `http_bind_addr` | Address and port of the plain-HTTP side (default `[::]:80`). The example uses `0.0.0.0:80`. |
| `[tls]` `https_bind_addr` | The relay itself (default: same address, port 443). |
| `[tls]` `hostname`, `contact`, `cert_mode`, `cert_dir` | Domain, e-mail, `"LetsEncrypt"` (or `"Manual"` with your own certificate files, or `"Reloading"`), certificate cache folder. |
| `enable_quic_addr_discovery` | Off by default. When on (needs TLS), listens on UDP 7842 and tells computers their public address, which helps them find a direct path. Whether Inkboard uses it with a custom relay is **not verified**; the relay works without it (our tests). |
| `enable_metrics` | On by default, on port 9090. The example turns it off; if you need metrics, never expose that port to the Internet. |
| `[limits.client.rx]` | Rate limit per connection (see next section). |
| `access` | Access control (allowlist, denylist, shared token, HTTP check). **Leave the default**; Inkboard's compatibility with these restrictions is **not verified**. |

The upstream project says that the full list of options is not published as documentation and refers to the source (`iroh-relay/src/main.rs`).

## Security, rate limits and abuse

- **Anybody who knows your relay's address can use it** as a relay (with the default access control): the relay does not know who the traffic belongs to and cannot read it. The risk is therefore bandwidth and CPU consumed by strangers, not leaked boards.
- **Rate limits are off by default** in the program. The example sets a per-connection limit in `[limits.client.rx]` (`bytes_per_second`, optional `max_burst_bytes`): the relay reads each connection at most at that speed (data is slowed down, not dropped; iroh endpoints log a notice that the relay is rate-limiting them). Adjust the value to your machine's bandwidth. The limits on new connections (`accept_conn_limit`, `accept_conn_burst`) have, according to the iroh documentation, no effect yet.
- Complement it with the **bandwidth cap and firewall of your provider**, and watch the traffic.
- **Metrics off or private**, as in the example. The container runs as root by default (the image sets no user): the example adds `no-new-privileges`; keep the machine updated and the relay alone on it if you can.
- **Logs**: the example limits the size of the container logs. The program writes to the standard error; what it logs by default (for example, whether IP addresses appear) has **not** been checked here. Check it before promising anything to your users.
- Your relay sees IP addresses, times and volumes: tell your users, as you would for any service.

## Backup

**Nothing to back up.** The relay holds no board and no account. Only the certificate cache is kept, and it is rebuilt automatically. Keep `relay.toml` (it is your configuration).

## Update

1. Change the image tag in `docker-compose.yml` (for example `v1.4.1` to a newer 1.x tag).
2. `docker compose pull && docker compose up -d`.

Follow the minor versions (a minor is fully supported for about three months). Sessions in progress are cut during the restart and the guests' "Reconnecting…" takes over (a cut of a few seconds to a minute is absorbed, see [SHARING.md](SHARING.md)); prefer updating when nobody is working.

## Use it from Inkboard

Inkboard can use a **custom list of relays** instead of the public ones. When you host a session with your relay, the link you share carries your relay's address: **guests do not need to configure anything**, they follow the link (verified in our tests with a local relay: the guest takes its relay from the link). If your guests are on a restricted network, the domain to ask the IT department to allow is your relay's domain (shown by the error window and by "Copy the diagnostic", see [SHARING.md](SHARING.md#restricted-networks-company-school)).

> **To be completed by lot R.** The exact setting (where it is, what it is called, how the list is typed or stored, how to go back to the public relays) is implemented by another work package and is **not described here on purpose**. Describe it here once it exists: location in the interface or in a file, format of the list, default, and a test (start a session, check that the link contains your domain).

What the code layer already supports, so that you know what to expect: a setting "default relays / custom list / no relay" exists in the network library; with a custom list, the relays of the link are used as well. That the link's relay address is arbitrary is a point the security review looks at.

## Limits (honest list)

- Not tried: a real VPS, a real domain, a real Let's Encrypt certificate, load with 20 or more people, the diagnostic button against your relay through a real company gateway.
- A relay does not fix a network that refuses your domain: if the company does not allow it, the session will not work through that network.
- A relay is not needed on a local network or a VPN where the computers reach each other directly (this has not been verified on two real machines).
- The browser version of Inkboard (planned) will need a relay with TLS.
