FROM docker.io/library/golang:1.25-alpine AS build
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY *.go ./
RUN CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags='-s -w' -o /runner .
FROM scratch
COPY --from=build /runner /runner
COPY --from=build /etc/ssl/certs/ca-certificates.crt /ca.crt
ENTRYPOINT ["/runner"]
