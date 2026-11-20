Three small jobs, one in each folder:

1. In `report/`: add a --json flag to report.py that prints the counts as a JSON object instead of the text lines. Without the flag the output must stay exactly as it is.
2. In `pager/`: paging is broken: the last item of every page goes missing. Fix it.
3. In `settings/`: add a timeout setting: it comes from the `timeout` key of settings.json, defaulting to 30, is carried by Config, and Client.request passes it to the transport as the timeout keyword.
