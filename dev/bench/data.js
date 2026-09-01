window.BENCHMARK_DATA = {
  "lastUpdate": 1788302217679,
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
      },
      {
        "commit": {
          "author": {
            "email": "5751456+admercs@users.noreply.github.com",
            "name": "Adam Erickson",
            "username": "admercs"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "f129e3136f78029991487d7a1e69d2fe69531e80",
          "message": "fix(ci): the Benchmarks job has never run to completion (#74)\n\n* fix(ci): the Benchmarks job has never run to completion\n\n#71 fixed the missing protoc, which was real, and revealed the next\nfailure rather than the last one:\n\n  fatal: couldn't find remote ref gh-pages\n  Error: The process 'git.exe' failed with exit code 128\n\ngithub-action-benchmark is configured with auto-push: true, which stores\nhistory on a gh-pages branch. This repository has none -- git ls-remote\n--heads origin gh-pages returns nothing -- so the action fails in its\nfirst git step, before a single result is recorded.\n\nTurning off auto-push and skipping the gh-pages fetch lets the benchmarks\nrun and report, which is the part that has been missing since the job was\nwritten.\n\nWhat this deliberately does not do is create the gh-pages branch. That is\nthe other valid fix, and arguably the intended design: the job also sets\nalert-threshold, comment-on-alert and alert-comment-cc-users, none of\nwhich mean anything without stored history. But creating that branch on a\npublic repository has GitHub Pages implications, and that should be a\ndecision rather than a side effect of turning a red job green. The\ncomment says which flags to flip once it exists.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\n\n* fix(ci): create the branch the Benchmarks job stores its history on\n\nCorrecting my own first attempt at this. I turned auto-push off to avoid\ncreating a gh-pages branch, on the grounds that it might have GitHub\nPages implications for a public repository. Two things were wrong with\nthat.\n\nIt did not work. skip-fetch-gh-pages skips the fetch, not the switch, so\nthe action still ran `git switch gh-pages` and failed with \"invalid\nreference: gh-pages\" -- save-data-file defaults to true and needs the\nbranch to write to. Disabling the fetch addressed the symptom I had seen\nrather than what the action actually does.\n\nAnd the caution was unfounded: GitHub Pages is not enabled for this\nrepository (the API returns 404 for it), so a branch of that name\npublishes nothing. Enabling Pages would remain a deliberate, separate\nact.\n\nSo the branch now exists, as an orphan carrying only dev/bench data and\nno source, and auto-push goes back to true -- which is what the job was\nalways configured for. alert-threshold, comment-on-alert and\nalert-comment-cc-users only mean something with stored history, and\nturning that off would have left three settings describing behaviour that\ncould not happen.\n\nThe comment records one more thing worth knowing: that branch must not be\nprotected. The action commits to it directly, and a pull-request rule\nbreaks it -- exactly the mistake the CLA workflow made by pointing at\nmaster.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\n\n---------\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-01T15:16:59-07:00",
          "tree_id": "e6149317f5c29ee9e129ec647029e86801990a3f",
          "url": "https://github.com/nervosys/HyperMachine/commit/f129e3136f78029991487d7a1e69d2fe69531e80"
        },
        "date": 1788302214269,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 516.949,
            "range": "+/- 1.672",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3391.523,
            "range": "+/- 12.916",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 380.51,
            "range": "+/- 1.175",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1008.092,
            "range": "+/- 4.949",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 354.284,
            "range": "+/- 0.855",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12619.59,
            "range": "+/- 66.775",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 583.039,
            "range": "+/- 1.533",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 7169.296,
            "range": "+/- 22.308",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 446.751,
            "range": "+/- 1.889",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1108.482,
            "range": "+/- 5.59",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 414.343,
            "range": "+/- 2.625",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12712.576,
            "range": "+/- 44.828",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2515.085,
            "range": "+/- 9.014",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 107.408,
            "range": "+/- 2.035",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 117.728,
            "range": "+/- 0.645",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1167.461,
            "range": "+/- 8.324",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2017.14,
            "range": "+/- 56.956",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 623.548,
            "range": "+/- 5.274",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 795.607,
            "range": "+/- 6.545",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 936.649,
            "range": "+/- 1.675",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10718.027,
            "range": "+/- 12.249",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 440.065,
            "range": "+/- 0.951",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2873.619,
            "range": "+/- 1.993",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 319.285,
            "range": "+/- 0.937",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 74094.245,
            "range": "+/- 755.882",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12657.504,
            "range": "+/- 136.223",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 356.332,
            "range": "+/- 1.043",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.533,
            "range": "+/- 0.079",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 93.323,
            "range": "+/- 0.243",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.699,
            "range": "+/- 0.105",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1422.621,
            "range": "+/- 6.936",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.247,
            "range": "+/- 0.123",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2141.763,
            "range": "+/- 16.02",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16409.829,
            "range": "+/- 65.894",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 735.321,
            "range": "+/- 0.798",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 663784.168,
            "range": "+/- 1095.885",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10448.188,
            "range": "+/- 13.686",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 251.03,
            "range": "+/- 0.729",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2685.446,
            "range": "+/- 1.519",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 129.126,
            "range": "+/- 0.941",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41532.86,
            "range": "+/- 60.135",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2247.206,
            "range": "+/- 6.913",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2037068.96,
            "range": "+/- 12139.179",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31285.301,
            "range": "+/- 119.927",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 802.592,
            "range": "+/- 4.258",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8065.123,
            "range": "+/- 29.238",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 298.696,
            "range": "+/- 1.778",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 125559.94,
            "range": "+/- 727.557",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7356.742,
            "range": "+/- 68.487",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8260.416,
            "range": "+/- 81.528",
            "unit": "ns"
          }
        ]
      }
    ]
  }
}