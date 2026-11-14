Add a timeout setting: it comes from the `timeout` key of settings.json, defaulting to 30, is carried by Config, and Client.request passes it to the transport as the timeout keyword.
