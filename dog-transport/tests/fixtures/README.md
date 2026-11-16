# Previous-release serialization fixture

`iroh-0103-postcard-113.tar.gz` contains only synthetic data generated with the
unmodified registry iroh-blobs 0.103.1 and postcard 1.1.3 (heapless 0.7 defaults).
It contains a closed, flushed filesystem store with named tags and blob sizes
0, 17, 16385 and 131073, plus golden Postcard encodings of a payload with tenant,
sequence, byte vector and optional text fields. Sequence boundaries include
0, 127, 128 and u64::MAX. Each byte vector is `index % 251`.

The compatibility test extracts a copy into a temporary directory, reads the
old hashes/data, compares both fixed-buffer and allocated encoding byte for byte,
then writes, flushes and reopens the current store. It never changes this archive
or an existing user store. The old packages are not needed by CI or the workspace.

Archive SHA256: `3102603ea534c82c59c144dca70e9dfb48a0e2680dabc1554a3f5008f663f5ab`.
