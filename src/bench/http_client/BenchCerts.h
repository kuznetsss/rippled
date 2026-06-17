#pragma once

#include <string_view>

// Self-signed certificate fixtures baked into the benchmark so it is fully
// self-contained — no external files, no certificate generation at runtime.
//
//   - The server presents `kServerChainPem` (leaf + CA) with `kServerKeyPem`.
//   - Both clients trust `kCaCertPem` as their sole CA root.
//
// A proper two-cert chain is used on purpose: a CA (the trust anchor) and a
// distinct leaf signed by it. rustls/webpki rejects a self-signed certificate
// that is simultaneously the trust anchor AND the presented end-entity cert
// (OpenSSL tolerates it, rustls does not), so the leaf must be a separate cert.
// The leaf carries `CN=127.0.0.1`, `subjectAltName=IP:127.0.0.1`, `CA:FALSE`
// and `extendedKeyUsage=serverAuth` (the serverAuth EKU is also required by
// webpki on the end-entity certificate). Generated with OpenSSL and only ever
// used against loopback in this tool.
namespace bench::certs {

// CA / trust anchor — installed as the sole root in both clients.
inline constexpr std::string_view kCaCertPem = R"PEM(-----BEGIN CERTIFICATE-----
MIIDJzCCAg+gAwIBAgIUdK3aycXeht3wp25W2wFHE6aoWo4wDQYJKoZIhvcNAQEL
BQAwGzEZMBcGA1UEAwwQcmlwcGxlZC1iZW5jaC1jYTAeFw0yNjA2MTcxMDQ5MTda
Fw0zNjA2MTQxMDQ5MTdaMBsxGTAXBgNVBAMMEHJpcHBsZWQtYmVuY2gtY2EwggEi
MA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQDAaKstHbM5KRH7PKMykdiiSQqf
fDVluG8wbIe3+dkzNrJRAkkQDrzhcUd/8jcKxDl7enqGBLiqfn5mmkpPvSWQP6rt
JvcgvqhDs5oBRMwDWoM1bHv9NFpV472sWI5ZuOqPPJX9cbNr1fJgEC4MqBQTsKI/
J2wr+VmYlpoDBRZeh/NLWUSZwyFFySHBq9WYwi6dhRF33dNZmHGivIjNUGGCmZSp
gnHlszS6zJQhh/1vMi0q7zkMJd+fpTC21w+FBpDzqGzUPo7Xh94ob7UTlMf97XqO
pdXzfiS6S3UNDIKwMAJBW0ED2grcb92u+5uByumf0+hHi7jylCJbUaFecOR7AgMB
AAGjYzBhMB0GA1UdDgQWBBT1Fq9f+2u39V+zM3fT99g7OPA18DAfBgNVHSMEGDAW
gBT1Fq9f+2u39V+zM3fT99g7OPA18DAPBgNVHRMBAf8EBTADAQH/MA4GA1UdDwEB
/wQEAwIBBjANBgkqhkiG9w0BAQsFAAOCAQEAckp9oIhtAVgnOwFPoJl1wUpCCT+o
szrHKwwzIV6+9VRbsATxEGGhGYwbYXvPkO+QM/tgRFIAftZaCMXeXM8sCOUYG0mM
IGTYA4daJH3pU3T/cNmHdTXycYpGZdO7kljkODZvBZJZruOmpKuXIPd2ct8Ye9mx
Wi6+d5o4uWAcCi5hiEvgiC6AnhQL7ENltw2wVfdtjqlITdTD+fRT9+NXiO1IdyRj
h4XvYn7/IMpPD6nBEU8J3J64cNyYBKZYM0+TU+7lpfXAn5+d6+B9XTotVtTDG1aM
/UCXyPyGwWaHCm/W9Me82cg5eoYV5cQmEBk0Qjy4mgxV3pZdPSiQVnNWMg==
-----END CERTIFICATE-----
)PEM";

// Server certificate chain: leaf (CN=127.0.0.1) followed by the CA.
inline constexpr std::string_view kServerChainPem = R"PEM(-----BEGIN CERTIFICATE-----
MIIDRTCCAi2gAwIBAgIUVS8roZbwYFhutain/FjqkzcUyzkwDQYJKoZIhvcNAQEL
BQAwGzEZMBcGA1UEAwwQcmlwcGxlZC1iZW5jaC1jYTAeFw0yNjA2MTcxMDQ5MTda
Fw0zNjA2MTQxMDQ5MTdaMBQxEjAQBgNVBAMMCTEyNy4wLjAuMTCCASIwDQYJKoZI
hvcNAQEBBQADggEPADCCAQoCggEBAOCYHg0oHKIvLNJwfpvtZ9FCYRnnvH+IW/Kh
ejhLkX79iVAH5OhDc4t8+wbz7oGyMCNrlunNJ3xfYEcGOAEWcRiNyL9XpjW/N/Zs
JdtAt4cXcnQaRdINSJoNPDgIZZ24kmhyIGtQEf3rlk426HN+auGMViO6ya3eGhd4
+QVJVn7xYufNWMCP3JdvtOPhlxXdanqL3diQLkbDI1EmuGVpV30b2dymx5kG1CR7
7mlXec99SVZAxI3ri2rKiiSzT5cw7clslcCVWcDGpUKlx6cEYyAhJ7SGQcAmxOYw
i3WPwRPTywthAayS4IdZXYxcFiE3Qkmp/Edc7eJ0GGCiu09avksCAwEAAaOBhzCB
hDAMBgNVHRMBAf8EAjAAMA4GA1UdDwEB/wQEAwIFoDATBgNVHSUEDDAKBggrBgEF
BQcDATAPBgNVHREECDAGhwR/AAABMB0GA1UdDgQWBBTJcbGuGAagK27tXHQJfSVp
b6Il9jAfBgNVHSMEGDAWgBT1Fq9f+2u39V+zM3fT99g7OPA18DANBgkqhkiG9w0B
AQsFAAOCAQEAhbj7qfHy6smyAVr+TdlSRyX7N62oZhnPFhX7rBGWA2fOaha1HlCX
gBoQvlt1WcYYgaY+g2DtADyCDNPdhK21ozD9eW1Lo2jxcVeiM0K4fjPOqo54r3DH
N0VJwOkZ26P2o7BWE+29DX2zPe0CUkYn4/36vqh7SHsazKSqP3VJWCM0F3yVZx37
85DPzqcNQpKYZdHVNhzCHY5HvxcP/21V3OkgnK0BOGqUH+GfRoesfuXknCaE7vHk
tjt98pMMy/Puq8KUJmpLeW2+PC2xkeieV0wYKdbUq9yhtFyJoweT0sFnhtqRxcZb
pHMVn7E6Oi8Afyy7saTYyEBc67qlGS8boQ==
-----END CERTIFICATE-----
-----BEGIN CERTIFICATE-----
MIIDJzCCAg+gAwIBAgIUdK3aycXeht3wp25W2wFHE6aoWo4wDQYJKoZIhvcNAQEL
BQAwGzEZMBcGA1UEAwwQcmlwcGxlZC1iZW5jaC1jYTAeFw0yNjA2MTcxMDQ5MTda
Fw0zNjA2MTQxMDQ5MTdaMBsxGTAXBgNVBAMMEHJpcHBsZWQtYmVuY2gtY2EwggEi
MA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQDAaKstHbM5KRH7PKMykdiiSQqf
fDVluG8wbIe3+dkzNrJRAkkQDrzhcUd/8jcKxDl7enqGBLiqfn5mmkpPvSWQP6rt
JvcgvqhDs5oBRMwDWoM1bHv9NFpV472sWI5ZuOqPPJX9cbNr1fJgEC4MqBQTsKI/
J2wr+VmYlpoDBRZeh/NLWUSZwyFFySHBq9WYwi6dhRF33dNZmHGivIjNUGGCmZSp
gnHlszS6zJQhh/1vMi0q7zkMJd+fpTC21w+FBpDzqGzUPo7Xh94ob7UTlMf97XqO
pdXzfiS6S3UNDIKwMAJBW0ED2grcb92u+5uByumf0+hHi7jylCJbUaFecOR7AgMB
AAGjYzBhMB0GA1UdDgQWBBT1Fq9f+2u39V+zM3fT99g7OPA18DAfBgNVHSMEGDAW
gBT1Fq9f+2u39V+zM3fT99g7OPA18DAPBgNVHRMBAf8EBTADAQH/MA4GA1UdDwEB
/wQEAwIBBjANBgkqhkiG9w0BAQsFAAOCAQEAckp9oIhtAVgnOwFPoJl1wUpCCT+o
szrHKwwzIV6+9VRbsATxEGGhGYwbYXvPkO+QM/tgRFIAftZaCMXeXM8sCOUYG0mM
IGTYA4daJH3pU3T/cNmHdTXycYpGZdO7kljkODZvBZJZruOmpKuXIPd2ct8Ye9mx
Wi6+d5o4uWAcCi5hiEvgiC6AnhQL7ENltw2wVfdtjqlITdTD+fRT9+NXiO1IdyRj
h4XvYn7/IMpPD6nBEU8J3J64cNyYBKZYM0+TU+7lpfXAn5+d6+B9XTotVtTDG1aM
/UCXyPyGwWaHCm/W9Me82cg5eoYV5cQmEBk0Qjy4mgxV3pZdPSiQVnNWMg==
-----END CERTIFICATE-----
)PEM";

// Private key for the leaf certificate above.
inline constexpr std::string_view kServerKeyPem = R"PEM(-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDgmB4NKByiLyzS
cH6b7WfRQmEZ57x/iFvyoXo4S5F+/YlQB+ToQ3OLfPsG8+6BsjAja5bpzSd8X2BH
BjgBFnEYjci/V6Y1vzf2bCXbQLeHF3J0GkXSDUiaDTw4CGWduJJociBrUBH965ZO
NuhzfmrhjFYjusmt3hoXePkFSVZ+8WLnzVjAj9yXb7Tj4ZcV3Wp6i93YkC5GwyNR
JrhlaVd9G9ncpseZBtQke+5pV3nPfUlWQMSN64tqyooks0+XMO3JbJXAlVnAxqVC
pcenBGMgISe0hkHAJsTmMIt1j8ET08sLYQGskuCHWV2MXBYhN0JJqfxHXO3idBhg
ortPWr5LAgMBAAECggEAALCxXxcDsgnvbR3+MWdtcNh0FDhNLqmtmQvQYRzAg+PQ
NPymjpiX36SaZxBCq4VUvFSasxtGS4JuH1l5VRzbR6DhLlFsrG0MrbcDuCwAf1Um
50c+y3loPVM0GujR30Plc8dio83bVIIxC8cnj3EOipvjGLlLNe9mscHym8ocpQZg
q7uykxRjOBsvR7sscd++MdQCh3DpqONJTkIAJLxaz2rg+byGtRrsINUi4dqD0JKM
BKzZx2YoG7nwiPcvfkZoEZOZj2gQTM0+tXt2R8GSzWbx1H9Ki85/qh6kXxOmxLBj
YK2M3zuAaKdRugsih0PoAlTYTcL/DHwKAVSjRjzlQQKBgQDyh8qMCc8moSL/X0Cm
TUvJlKRDOfHl0woGgtmwnbA6tkWQ+f2hsonpQFgJJvInoyMd/oWEsTgN0uwLwe4U
eXCx/BLewHR80wemYi4ruyKegrDVdRIYOe1nArxsjxSeWBDV/V70Zl9PJCD/imEM
2Q+fgA8gQ/doWT1FHXhat058TwKBgQDtEVDE/dNJIvclDnGH5G2BSYCpJK4CjQM2
rE5ZTvB1GvH4zkmtMU43HFVoD0sHEYjIbdGNPJUTX2PexiLXaKvcYZLrBHyTT2rK
LDPaMdI04VQKQqq4DQNcNeKBEGAVGWv+OZsyuVP1AOmF15YjqqhKZtjNo68mkzRI
9VNDmzizRQKBgCXxYpnICxWDDiOftlCONTYjQBOYZCTNgHsGS6Ja+TAmRfnpcmmy
serA+0TrR+U1m4/cuuzIgPmArxcjzuh7G0ttIVKiD1db+I1qPMjwrPjZf2rVtu/9
WAvOnMXrGJGxO2kPC6T2wyBxiYwRDR/icZRFh5hHqdQ0aeZ/Ns4ScX/3AoGAHL4+
tsO3vGRa5slLhphxln1c9iUWXATQ4O4fScDCctBjijEoybDytMfgqw8/n4nGtdZq
098GjCTCrR4E120/eSbzcz9GA25bmkultczYmcTANcZDgLFDOQjnf5KGs8gzSc2e
PZYu0cPcjHfJImXspai2nKg98kViz32/LLFDPKkCgYEA7gkk+SlHlvgj+vvjrPeq
u52lt7M4gx6Kp/3PQv5FFx9WMyVcQuTVW281KOOLvFhdLwJk1eS4sLchp0I3ryI1
xM6SYpoWarlrT8fKOb0OZGvVIXzDD/CyGGjsMczLRJnc7/1GiyDcp2iYbTw/QuWK
CD+tqlz7NDUSokU/apqZVWM=
-----END PRIVATE KEY-----
)PEM";

}  // namespace bench::certs
