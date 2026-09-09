"""Credential-blind verification of the existing Pi local-provider registry."""

import hashlib
import ipaddress
import json
import os
from pathlib import Path
import socket
from urllib.parse import urlsplit


PRIVATE_NETWORKS = tuple(
    ipaddress.ip_network(value)
    for value in (
        "127.0.0.0/8",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "100.64.0.0/10",
        "::1/128",
        "fc00::/7",
    )
)


def active_pi_registry():
    """Match Pi's inherited agent directory without changing its environment."""
    configured = os.environ.get("PI_CODING_AGENT_DIR")
    directory = (
        Path(configured).expanduser() if configured else Path.home() / ".pi" / "agent"
    )
    return (directory / "models.json").resolve()


def verify_local_route(registry_path, model):
    """Require an exact custom model on a private endpoint in Pi's active file.

    This verifies configuration, not packet capture or the gateway's upstream
    model placement. Credentials, headers and command-valued keys are ignored.
    """
    if registry_path is None:
        raise RuntimeError(
            "--route local-gateway requires --pi-models-file pointing to Pi's existing active models.json"
        )
    path = registry_path.resolve()
    if path != active_pi_registry():
        raise RuntimeError("--pi-models-file is not Pi's active inherited models.json")
    if not isinstance(model, str) or "/" not in model:
        raise RuntimeError(
            "local-gateway evidence requires an exact provider/model identifier"
        )
    provider_id, model_id = model.split("/", 1)
    try:
        raw = path.read_bytes()
        provider = json.loads(raw)["providers"][provider_id]
        matches = [
            item for item in provider.get("models", []) if item.get("id") == model_id
        ]
        if len(matches) != 1:
            raise ValueError("model not uniquely declared")
        # Fail closed on endpoint overrides outside the supported registry shape.
        if "baseUrl" in matches[0] or "baseUrl" in provider.get(
            "modelOverrides", {}
        ).get(model_id, {}):
            raise ValueError("unsupported model endpoint override")
        endpoint = urlsplit(provider["baseUrl"])
        if (
            endpoint.scheme not in ("http", "https")
            or not endpoint.hostname
            or endpoint.username is not None
            or endpoint.password is not None
            or endpoint.query
            or endpoint.fragment
        ):
            raise ValueError("unsupported endpoint URL")
        addresses = {
            ipaddress.ip_address(item[4][0])
            for item in socket.getaddrinfo(
                endpoint.hostname,
                endpoint.port or (443 if endpoint.scheme == "https" else 80),
                type=socket.SOCK_STREAM,
            )
        }
        if not addresses or any(
            not any(address in network for network in PRIVATE_NETWORKS)
            for address in addresses
        ):
            raise ValueError("endpoint is not exclusively loopback/private/tailnet")
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        # Never echo config values: baseUrl or arbitrary JSON fields can contain secrets.
        raise RuntimeError(
            "cannot verify the exact Pi model on a private gateway endpoint from the supplied registry"
        ) from error
    return {
        "status": "verified_private_configuration",
        "registry_path": str(path),
        "registry_sha256": hashlib.sha256(raw).hexdigest(),
        "provider": provider_id,
        "model": model_id,
        "endpoint_origin": f"{endpoint.scheme}://{endpoint.netloc}",
        "resolved_addresses": sorted(str(address) for address in addresses),
        "transport_observed": False,
        "upstream_model_locality": "unverified",
    }
