ARG UBUNTU_VERSION=24.04
FROM ubuntu:${UBUNTU_VERSION}

SHELL ["/bin/bash", "-c"]

WORKDIR /workspace

COPY . /workspace

RUN bash /workspace/scripts/test-setup-scripts.sh
