# syntax=docker/dockerfile:1
#
# The Inkboard MCP server (`inkboard-mcp`) as a container image. It runs in "file mode": there is no window and no running
# application, so the agent creates and edits `.inkboard` boards in the mounted folder, imports Mermaid and exports.
#
#   docker build -f packaging/docker/mcp.Dockerfile -t inkboard-mcp .
#   docker run -i --rm -v "$PWD/boards:/data" inkboard-mcp
#
# The server speaks MCP over standard input/output (hence `-i`). `--root /data` confines every file it reads or writes to that folder.

FROM rust:1.99-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p inkboard-mcp

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home inkboard \
    && mkdir /data \
    && chown inkboard /data
COPY --from=build /src/target/release/inkboard-mcp /usr/local/bin/inkboard-mcp
USER inkboard
WORKDIR /data
VOLUME /data
ENTRYPOINT ["inkboard-mcp", "--no-live", "--root", "/data"]
