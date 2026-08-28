FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY lostoken-api /usr/local/bin/lostoken-api
RUN chmod 755 /usr/local/bin/lostoken-api
EXPOSE 8080
CMD ["/usr/local/bin/lostoken-api"]
