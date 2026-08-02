# Licence file format

The contract between two things that ship separately: the validator compiled into
every installed binary, and the signer running on the licence service. A client
that is already installed cannot be updated in lockstep with the server, so this
format is versioned and changes additively.

Published here so a buyer can verify their own file rather than take it on trust.

## Shape

Two base64url segments joined by a dot, no padding:

```
<base64url(payload)>.<base64url(signature)>
```

The payload is stored encoded, never re-serialised. Verification decodes the
first segment, checks the signature over **those exact bytes**, and only then
parses the JSON. Signing a parsed-and-re-serialised object is the usual way this
kind of format breaks: key order, whitespace and number formatting all differ
between languages, and the signature stops matching for reasons nobody can see.

**There is no algorithm field.** The algorithm is ed25519, hardcoded on both
sides. A token that announces its own algorithm invites the verifier to be
talked into `none`, which is the best known way to forge one of these.

## Payload

```json
{
  "v": 1,
  "id": "lic_2f8a...",
  "email": "buyer@example.com",
  "name": "Buyer Name",
  "issued": "2026-08-02T19:20:00Z",
  "updatesUntil": "2027-08-02T00:00:00Z"
}
```

| Field | Meaning |
| --- | --- |
| `v` | Format version. An unknown value is refused: the app says the licence needs a newer soquel rather than guessing. |
| `id` | The Polar licence key id. Carried for support, never checked offline. |
| `email` | Who bought it. Shown in the app so a shared file is at least attributable. |
| `name` | Optional. |
| `issued` | When the service signed this file. |
| `updatesUntil` | The last day of the update window. |

Dates are RFC 3339 in UTC.

## What the app checks

1. The signature verifies against the public key compiled into the binary.
2. `v` is a version this build understands.
3. The build's own release date is on or before `updatesUntil`.

Nothing else, and no network. A licence file never expires: `updatesUntil` bounds
*which builds it activates*, not how long it is valid. A buyer who lets the window
lapse keeps the last covered version working forever.

### The consequence for the release pipeline

Check 3 needs the running build to know when it was released, offline. That date
has to be baked in at build time, alongside the version. A binary that cannot say
when it was made cannot tell whether a licence covers it.

## Public key

```
mUac/9bOAvFXbUa/lZd5k3qoRjV6O09T1IVuge/rjLk=
```

Raw 32 bytes, base64. Compiled into every build; the matching private key signs
on the service and exists nowhere else.

## Verifying a file yourself

```bash
# Split the token, then check the signature over the payload bytes as they are.
printf '%s' "$LICENCE" | cut -d. -f1 | base64 -d --ignore-garbage > payload.bin
printf '%s' "$LICENCE" | cut -d. -f2 | base64 -d --ignore-garbage > sig.bin
openssl pkeyutl -verify -pubin -inkey licence-signing.pub.pem \
  -rawin -in payload.bin -sigfile sig.bin
```

`licence-signing.pub.pem` is the key above in PEM form; any ed25519 verifier
works, the format carries nothing proprietary.

## What this format deliberately does not do

**No machine binding.** Polar counts activations; the file is per purchase, not
per machine, so moving to a new laptop is a copy rather than a support ticket.

**No revocation.** Offline validation cannot revoke. Revoking a key in Polar
stops future activations; a file already issued keeps working. That is the price
of a check that runs with the network unplugged, and it is the same trade the
rest of the app makes.
