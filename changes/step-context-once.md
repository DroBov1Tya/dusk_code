Changed
A step reuses the context the size check after the previous step just read, instead of loading the whole thread again, unless a message joined in between.
