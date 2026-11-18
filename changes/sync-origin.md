Changed
The protocol is version 20: the Workshop's `StoreRequest::Screenshot` and `Response::Media` changed its shape. A screenshot is in the same scope area as a cover, reading the store, so a browser may fetch it and a scope bound to chats may not.
