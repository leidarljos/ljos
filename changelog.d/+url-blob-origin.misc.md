The lock takes url 2.5.9 and form_urlencoded 1.2.3, so cargo deny no longer reports `RUSTSEC-2026-0592`, a stack overflow in `Url::origin()` on nested `blob:` URLs.
