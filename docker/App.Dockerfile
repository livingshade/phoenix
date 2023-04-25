FROM ubuntu:22.04 as build

RUN apt update

RUN DEBIAN_FRONTEND=noninteractive apt install -y \
        --allow-unauthenticated --allow-downgrades --allow-change-held-packages --no-install-recommends \
        sudo \
        build-essential \
        tzdata \
        ca-certificates \
        openssh-server \
        curl \
        wget \
        bison \
        iproute2 \
        iputils-ping dnsutils pciutils \
        iftop \
        unzip \
        cmake \
        git \
        lsb-release \
        numactl

RUN DEBIAN_FRONTEND=noninteractive apt install -y \
        --allow-unauthenticated --allow-downgrades --allow-change-held-packages --no-install-recommends \
        libclang-dev libnuma-dev librdmacm-dev libibverbs-dev protobuf-compiler

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN echo 'source $HOME/.cargo/env' >> $HOME/.bashrc
ENV PATH="/root/.cargo/bin:${PATH}"
RUN cargo install cargo-make

WORKDIR /phoenix
COPY . ./

RUN cargo make
WORKDIR /phoenix/experimental/mrpc
RUN cat load-mrpc-plugins.toml >> /phoenix/phoenix.toml
RUN cargo make

RUN cargo build --release --workspace -p rpc_echo
WORKDIR /phoenix
RUN cargo make deploy-plugins

FROM ubuntu:22.04

WORKDIR /phoenix

RUN apt update && apt install libnuma-dev -y

COPY --from=build /phoenix/experimental/mrpc/target/release/rpc_echo_client .
COPY --from=build /phoenix/experimental/mrpc/target/release/rpc_echo_client2 .
COPY --from=build /phoenix/experimental/mrpc/target/release/rpc_echo_server .
COPY --from=build /phoenix/experimental/mrpc/target/release/rpc_echo_frontend .