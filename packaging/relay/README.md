# Self-hosted relay kit

Example files to run your own [`iroh-relay`](https://hub.docker.com/r/n0computer/iroh-relay) (the connection service Inkboard's live sharing uses). The relay is stateless and only carries encrypted data. Full guide: [docs/RELAY.md](../../docs/RELAY.md) (French: [docs/fr/RELAY.md](../../docs/fr/RELAY.md)).

- `docker-compose.yml`: official image, ports 80 and 443, certificate cache volume.
- `relay.toml`: commented configuration (automatic Let's Encrypt certificate, per-connection rate limit, metrics off).

## Quick start

1. A machine with a public IP address and a DNS name pointing at it; TCP 80 and 443 open.
2. Edit `relay.toml`: `hostname` (your domain) and `contact` (your e-mail).
3. `docker compose up -d`
4. Check: `curl --noproxy '*' --fail --show-error https://<your-domain>/healthz`

Update: change the image tag in `docker-compose.yml`, then `docker compose pull && docker compose up -d`.

How to make Inkboard use this relay: see "Use it from Inkboard" in the guide (the setting is delivered separately).

Status: the files were checked for syntax (`docker compose config`, TOML parser). A full run with a real domain and certificate has **not** been tried.
