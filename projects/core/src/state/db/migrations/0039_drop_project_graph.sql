-- The project graph is gone: the model maps a project by file names, computed when it
-- asks, and reads what files hold itself. Its rows could all be built again from the
-- folder, so dropping them loses nothing.
DROP TABLE IF EXISTS graph_refs;
DROP TABLE IF EXISTS graph_symbols;
DROP TABLE IF EXISTS graph_files;
