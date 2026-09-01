window.BENCHMARK_DATA = {
  "lastUpdate": 1788301952591,
  "repoUrl": "https://github.com/nervosys/HyperMachine",
  "entries": {
    "HyperMachine Benchmarks": [
      {
        "commit": {
          "author": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "committer": {
            "name": "nervosys",
            "username": "nervosys"
          },
          "id": "e4ed78673bc9edd0f7927779c554fa87180b910c",
          "message": "fix(ci): the Benchmarks job has never run to completion",
          "timestamp": "2026-09-01T22:01:44Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/74/commits/e4ed78673bc9edd0f7927779c554fa87180b910c"
        },
        "date": 1788301948672,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 513.95,
            "range": "+/- 15.822",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2706.851,
            "range": "+/- 12.174",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 357.307,
            "range": "+/- 1.066",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 834.581,
            "range": "+/- 6.18",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 332.108,
            "range": "+/- 0.52",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 9846.936,
            "range": "+/- 17.971",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 514.048,
            "range": "+/- 1.338",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 2962.782,
            "range": "+/- 7.463",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 403.359,
            "range": "+/- 0.378",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 945.562,
            "range": "+/- 4.041",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 376.301,
            "range": "+/- 0.588",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 15908.524,
            "range": "+/- 54.162",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2102.122,
            "range": "+/- 2.447",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 83.971,
            "range": "+/- 0.181",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 92.327,
            "range": "+/- 0.311",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 995.135,
            "range": "+/- 4.234",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1602.038,
            "range": "+/- 11.688",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 560.345,
            "range": "+/- 1.385",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 703.506,
            "range": "+/- 1.817",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 821.846,
            "range": "+/- 1.12",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9329.063,
            "range": "+/- 11.478",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 387.446,
            "range": "+/- 0.34",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2514.211,
            "range": "+/- 1.37",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 280.075,
            "range": "+/- 0.472",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 58860.446,
            "range": "+/- 198.278",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8930.957,
            "range": "+/- 68.706",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 304.474,
            "range": "+/- 1.079",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 13.294,
            "range": "+/- 0.181",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 76.813,
            "range": "+/- 0.098",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 16.877,
            "range": "+/- 0.016",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1212.688,
            "range": "+/- 2.965",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 27.051,
            "range": "+/- 0.076",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1565.768,
            "range": "+/- 5.828",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11813.891,
            "range": "+/- 35.296",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 650.156,
            "range": "+/- 0.484",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 579924.496,
            "range": "+/- 941.664",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9125.918,
            "range": "+/- 4.934",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 215.244,
            "range": "+/- 0.312",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2345.035,
            "range": "+/- 0.876",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 108.581,
            "range": "+/- 0.279",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 36247.847,
            "range": "+/- 16.885",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2004.663,
            "range": "+/- 23.083",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1733955.437,
            "range": "+/- 4967.609",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 27314.991,
            "range": "+/- 53.548",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 687.022,
            "range": "+/- 1.226",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7182.414,
            "range": "+/- 55.111",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 253.017,
            "range": "+/- 0.851",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 109233.863,
            "range": "+/- 350.639",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5221.351,
            "range": "+/- 19.21",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5893.669,
            "range": "+/- 28.357",
            "unit": "ns"
          }
        ]
      }
    ]
  }
}