# MCP server image: scry binary + stdio MCP server bridged to
# streamable HTTP by supergateway. Override --streamableHttpPath in
# compose to mount it under a secret path.
FROM rust:1-slim AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM node:22-slim
RUN npm install -g supergateway
COPY --from=build /src/target/release/scry /usr/local/bin/scry
WORKDIR /app
COPY mcp/package.json mcp/package-lock.json ./
RUN npm ci --omit=dev
COPY mcp/index.js ./
ENV SCRY_BIN=/usr/local/bin/scry
EXPOSE 8000
CMD ["supergateway", "--stdio", "node /app/index.js", "--outputTransport", "streamableHttp", "--port", "8000", "--streamableHttpPath", "/mcp"]
