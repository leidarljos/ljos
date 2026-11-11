`ljos memory export --out` writes the file in place when rename fails with an input/output error (EIO), a cross-device link (EXDEV), an operation not permitted (EPERM), or an operation not supported (ENOTSUP).
The file is read back, and the temporary file is removed.
