# glances-rs on Fedora (multi-stage).
#
# Builder compiles from source with the Fedora Rust toolchain
# (pure-std, zero crates: rustc + cargo is the whole build closure).
# Runtime is fedora-minimal plus the binary's one dynamic dep
# outside glibc (libgcc). No nvidia-smi inside: NVIDIA util/mem/
# temp read Null in containers (documented gap).
#
# Run against the HOST (a monitor in a bare container only sees
# itself):
#   podman run -d --name glances-rs --pid=host --net=host \
#     -v /sys:/sys:ro glances-rs:0.10.15 \
#     -w -B 100.117.155.12 --web-port 61212 -p 61213
#
# NVIDIA live stats need passthrough (CDI devices + the host's
# nvidia-smi binary; the image deliberately ships neither):
#   podman run -d --name glances-rs --pid=host --net=host \
#     -v /sys:/sys:ro --device nvidia.com/gpu=all \
#     -v /usr/bin/nvidia-smi:/usr/bin/nvidia-smi:ro \
#     glances-rs:0.10.15 \
#     -w -B 100.117.155.12 --web-port 61212 -p 61213
ARG FEDORA_VERSION=44

FROM registry.fedoraproject.org/fedora:${FEDORA_VERSION} AS builder
RUN dnf install -y rust cargo && dnf clean all && rustc --version
WORKDIR /src
COPY . .
RUN cargo build --release --offline && cp target/release/glances-rs /glances-rs

FROM registry.fedoraproject.org/fedora-minimal:${FEDORA_VERSION}
RUN microdnf install -y libgcc && microdnf clean all
COPY --from=builder /glances-rs /usr/local/bin/glances-rs
EXPOSE 61208-61209
ENTRYPOINT ["/usr/local/bin/glances-rs"]
CMD ["-w"]
