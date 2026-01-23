# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue, please report it responsibly.

### How to Report

**DO NOT** open a public GitHub issue for security vulnerabilities.

Instead, please report security issues via GitHub's private vulnerability reporting:

1. Go to the [Security tab](https://github.com/limaronaldo/engram/security)
2. Click "Report a vulnerability"
3. Fill out the form with details

### What to Include

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

### Response Timeline

- **Initial Response**: Within 48 hours
- **Status Update**: Within 7 days
- **Resolution Target**: Within 30 days (depending on severity)

### What to Expect

1. **Acknowledgment**: We'll confirm receipt of your report
2. **Investigation**: We'll investigate and validate the issue
3. **Resolution**: We'll develop and test a fix
4. **Disclosure**: We'll coordinate disclosure timing with you
5. **Credit**: We'll credit you in the release notes (unless you prefer anonymity)

## Security Best Practices

### Cloud Storage

- Always enable encryption (`ENGRAM_CLOUD_ENCRYPT=true`) for cloud sync
- Use dedicated API keys with minimal permissions
- Rotate credentials regularly

### Database

- Protect your local database file with appropriate file permissions
- Back up your database regularly

### API Keys

- Store API keys in environment variables or `.env.local`
- Never commit credentials to version control

## Security Updates

Security updates will be released as patch versions and announced via GitHub Security Advisories.

Thank you for helping keep Engram secure!
