Added
The protocol is published as a JSON Schema in `docs/api/schema.json`, generated from the wire types, with the handshake types first. CI regenerates it and fails when it differs, and, while `OLDEST_PROTOCOL` is unchanged, when a property or variant was removed or a property became required, so the schema can only grow.
