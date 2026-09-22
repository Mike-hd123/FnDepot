FROM alpine

ARG TARGETPLATFORM
ENV TZ=Asia/Shanghai \
    APP_NAME=octopus

RUN apk add --no-cache alpine-conf ca-certificates su-exec && \
    /usr/sbin/setup-timezone -z Asia/Shanghai && \
    apk del alpine-conf && \
    rm -rf /var/cache/apk/* && \
    mkdir -p /app

COPY build/docker/${TARGETPLATFORM}/${APP_NAME} /app/${APP_NAME}
COPY LICENSE /app/LICENSE
COPY build/THIRD_PARTY_LICENSES.csv /app/THIRD_PARTY_LICENSES.csv
COPY scripts/dockerfile/entrypoint.sh /entrypoint.sh

RUN chmod +x /entrypoint.sh

CMD ["/entrypoint.sh"]
