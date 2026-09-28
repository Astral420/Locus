from sidecar.src.rpc.server import JsonRpcServer


def main() -> None:
    JsonRpcServer().serve_forever()


if __name__ == "__main__":
    main()
