"""Errors shared by sidecar modules."""

from __future__ import annotations

# JSON-RPC error code for "this install cannot do that" (missing optional
# dependency, missing binary or model). The host treats it as a skipped
# capability, not as a failure, and the sidecar logs one line instead of a
# traceback so a missing optional package never looks like a crash.
CAPABILITY_UNAVAILABLE = -32001


class CapabilityUnavailable(RuntimeError):
    """An optional capability is not installed or not configured."""
