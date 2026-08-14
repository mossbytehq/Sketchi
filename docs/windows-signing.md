# Windows release signing

Public Windows downloads must be signed with a certificate trusted by Windows.
The release workflow signs the client executable, server sidecar, portable
artifacts, MSI, and setup bootstrapper when `sign_windows` is enabled by the
release workflow.

Configure these GitHub Actions secrets before creating a release:

- `WINDOWS_SIGNING_CERTIFICATE_BASE64`: a base64-encoded `.pfx` certificate
  containing the private key and certificate chain.
- `WINDOWS_SIGNING_CERTIFICATE_PASSWORD`: the password for the `.pfx` file.

For example, create the base64 value locally with PowerShell:

```powershell
[Convert]::ToBase64String(
  [IO.File]::ReadAllBytes("Sketchi-code-signing.pfx")
)
```

Use a CA-issued OV certificate or a managed signing service such as Microsoft
Artifact Signing. A self-signed certificate is suitable only for private
testing and will still produce a strong SmartScreen warning for public users.

The workflow uses Microsoft's `signtool.exe` with SHA-256 signing and an RFC
3161 timestamp, then verifies every signature before the release artifacts are
assembled.
