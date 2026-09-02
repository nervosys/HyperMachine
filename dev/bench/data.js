window.BENCHMARK_DATA = {
  "lastUpdate": 1788390202501,
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
          "id": "d522bf63d936ddb67652f0c8114a961ad7c428f7",
          "message": "fix(ci): the CLA check stored signatures on a branch it cannot write to (#75)\n\nWith the action resolving for the first time (#71 pinned it to a tag that\nexists), it got far enough to reveal the next problem:\n\n  Error occurred when creating the signed contributors file: Repository\n  rule violations found. Changes must be made through a pull request.\n  Make sure the branch where signatures are stored is NOT protected.\n\nThe workflow set branch: \"master\", and master's ruleset requires a pull\nrequest for any change. The action records a signature by committing\nsignatures/cla.json to that branch, so the commit was refused. No amount\nof signing could have turned this check green -- the contributor comment\nwould have been accepted and then failed to record.\n\nPoints it at cla-signatures instead, a branch created for this and left\nunprotected, as the action's own error message asks for.\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-01T16:18:06-07:00",
          "tree_id": "248f97c0f502049cdcf3e846bbe1a070b25662f3",
          "url": "https://github.com/nervosys/HyperMachine/commit/d522bf63d936ddb67652f0c8114a961ad7c428f7"
        },
        "date": 1788305556155,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 543.144,
            "range": "+/- 2.018",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3393.619,
            "range": "+/- 8.181",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 469.828,
            "range": "+/- 5.946",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1019.256,
            "range": "+/- 3.233",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 419.79,
            "range": "+/- 10.915",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12430.69,
            "range": "+/- 49.682",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 609.306,
            "range": "+/- 0.996",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3738.524,
            "range": "+/- 12.277",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 475.159,
            "range": "+/- 1.418",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1173.708,
            "range": "+/- 7.105",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 432.458,
            "range": "+/- 1.567",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13208.65,
            "range": "+/- 41.477",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2664.626,
            "range": "+/- 9.187",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.563,
            "range": "+/- 0.396",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 116.258,
            "range": "+/- 0.841",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1244.224,
            "range": "+/- 5.354",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2002.907,
            "range": "+/- 9.702",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 655.879,
            "range": "+/- 2.595",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 876.3,
            "range": "+/- 4.56",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1053.255,
            "range": "+/- 1.284",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12003.115,
            "range": "+/- 5.898",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 497.592,
            "range": "+/- 0.708",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3245.542,
            "range": "+/- 2.802",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 367.55,
            "range": "+/- 4.294",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75580.92,
            "range": "+/- 126.088",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11416.087,
            "range": "+/- 88.684",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 394.58,
            "range": "+/- 0.635",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 18.351,
            "range": "+/- 0.293",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 103.711,
            "range": "+/- 0.236",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.024,
            "range": "+/- 0.089",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1574.917,
            "range": "+/- 4.514",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 36.083,
            "range": "+/- 0.618",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2084.47,
            "range": "+/- 6.842",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16227.831,
            "range": "+/- 198.192",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 846.987,
            "range": "+/- 1.366",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747075.342,
            "range": "+/- 327.893",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11811.008,
            "range": "+/- 6.316",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 283.629,
            "range": "+/- 0.295",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3041.56,
            "range": "+/- 3.884",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 140.872,
            "range": "+/- 0.386",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46734.87,
            "range": "+/- 27.919",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2559.429,
            "range": "+/- 18.016",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2251746.435,
            "range": "+/- 9324.857",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35101.279,
            "range": "+/- 104.818",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 899.457,
            "range": "+/- 3.992",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9126.163,
            "range": "+/- 56.06",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 332.791,
            "range": "+/- 2.431",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140128.364,
            "range": "+/- 493.72",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6885.94,
            "range": "+/- 69.242",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7532.698,
            "range": "+/- 47.716",
            "unit": "ns"
          }
        ]
      },
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
          "id": "dda660158b0660bad2f394af2729e31d0a2e8000",
          "message": "deps: update hashicorp/aws requirement from ~> 5.0 to ~> 6.62 in /deploy/terraform",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/72/commits/dda660158b0660bad2f394af2729e31d0a2e8000"
        },
        "date": 1788305683843,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 506.854,
            "range": "+/- 3.055",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3300.177,
            "range": "+/- 11.294",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 374.765,
            "range": "+/- 1.471",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 986.169,
            "range": "+/- 3.886",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 339.129,
            "range": "+/- 1.116",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12753.792,
            "range": "+/- 71.36",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 581.756,
            "range": "+/- 4.874",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6515.982,
            "range": "+/- 34.255",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 531.889,
            "range": "+/- 9.974",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1114.335,
            "range": "+/- 7.322",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 409.102,
            "range": "+/- 2.519",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13333.335,
            "range": "+/- 88.692",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2481.887,
            "range": "+/- 14.338",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.152,
            "range": "+/- 0.437",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 113.594,
            "range": "+/- 0.827",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1093.459,
            "range": "+/- 3.687",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1757.398,
            "range": "+/- 2.114",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 586.455,
            "range": "+/- 2.498",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 751.406,
            "range": "+/- 2.561",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 946.278,
            "range": "+/- 2.247",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10761.921,
            "range": "+/- 107.29",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 455.225,
            "range": "+/- 1.907",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2908.796,
            "range": "+/- 4.648",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 337.096,
            "range": "+/- 2.284",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72763.175,
            "range": "+/- 490.571",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12775.923,
            "range": "+/- 174.138",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 355.316,
            "range": "+/- 1.656",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.776,
            "range": "+/- 0.086",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 97.731,
            "range": "+/- 0.546",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.972,
            "range": "+/- 0.071",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1387.499,
            "range": "+/- 1.791",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 33.041,
            "range": "+/- 0.15",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2060.359,
            "range": "+/- 26.998",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 19594.605,
            "range": "+/- 548.605",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 733.324,
            "range": "+/- 0.736",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662771.679,
            "range": "+/- 381.827",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10446.827,
            "range": "+/- 21.359",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 249.37,
            "range": "+/- 0.663",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2694.683,
            "range": "+/- 2.135",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 127.244,
            "range": "+/- 0.135",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41469.119,
            "range": "+/- 33.74",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2264.794,
            "range": "+/- 11.227",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2099206.88,
            "range": "+/- 21624.098",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31661.829,
            "range": "+/- 152.627",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 811.799,
            "range": "+/- 4.166",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8091.481,
            "range": "+/- 29.302",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 300.272,
            "range": "+/- 1.889",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124603.241,
            "range": "+/- 430.643",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7238.834,
            "range": "+/- 21.745",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8238.362,
            "range": "+/- 69.303",
            "unit": "ns"
          }
        ]
      },
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
          "id": "adc2ee5e8128aa96ca7c86e0b35f1d73a98b5a9d",
          "message": "deps(deps): bump tock-registers from 0.9.0 to 0.10.1",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/48/commits/adc2ee5e8128aa96ca7c86e0b35f1d73a98b5a9d"
        },
        "date": 1788305852192,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 509.202,
            "range": "+/- 3.336",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3435.647,
            "range": "+/- 15.487",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 398.6,
            "range": "+/- 1.107",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1011.234,
            "range": "+/- 2.605",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 359.174,
            "range": "+/- 1.674",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12385.347,
            "range": "+/- 39.312",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 581.446,
            "range": "+/- 1.545",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6407.883,
            "range": "+/- 16.424",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 450.66,
            "range": "+/- 3.709",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1116.455,
            "range": "+/- 3.691",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 413.402,
            "range": "+/- 2.14",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12653.348,
            "range": "+/- 50.986",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2446.015,
            "range": "+/- 8.562",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.708,
            "range": "+/- 0.4",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 113.701,
            "range": "+/- 0.788",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1066.15,
            "range": "+/- 1.783",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1718.744,
            "range": "+/- 5.951",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 590.019,
            "range": "+/- 2.832",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 747.063,
            "range": "+/- 2.189",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 934.014,
            "range": "+/- 1.417",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10630.007,
            "range": "+/- 8.809",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 442.65,
            "range": "+/- 0.496",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2881.477,
            "range": "+/- 5.776",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 322.53,
            "range": "+/- 1.16",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 72738.695,
            "range": "+/- 391.148",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12971.38,
            "range": "+/- 266.891",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 352.23,
            "range": "+/- 0.808",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.652,
            "range": "+/- 0.1",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 93.676,
            "range": "+/- 0.293",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.561,
            "range": "+/- 0.047",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1396.667,
            "range": "+/- 3.471",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 31.772,
            "range": "+/- 0.101",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1941.686,
            "range": "+/- 8.511",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17489.81,
            "range": "+/- 344.25",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 730.412,
            "range": "+/- 0.97",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662907.903,
            "range": "+/- 885.859",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10434.363,
            "range": "+/- 10.23",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 246.605,
            "range": "+/- 0.426",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2682.061,
            "range": "+/- 2.442",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 125.007,
            "range": "+/- 0.216",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41662.016,
            "range": "+/- 149.987",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2238.889,
            "range": "+/- 5.072",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1995319.623,
            "range": "+/- 6784.926",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31474.844,
            "range": "+/- 215.025",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 797.423,
            "range": "+/- 3.558",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8076.132,
            "range": "+/- 34.276",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 299.961,
            "range": "+/- 2.683",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 125277.841,
            "range": "+/- 844.192",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7252.539,
            "range": "+/- 34.525",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8225.726,
            "range": "+/- 80.958",
            "unit": "ns"
          }
        ]
      },
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
          "id": "67faa0ec030673370880d7d57f7d6e54f646016d",
          "message": "deps: bump rust from 1.87-bookworm to 1.98-bookworm",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/62/commits/67faa0ec030673370880d7d57f7d6e54f646016d"
        },
        "date": 1788305952028,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 554.755,
            "range": "+/- 2.587",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3415.883,
            "range": "+/- 8.147",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 428.37,
            "range": "+/- 1.057",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1032.034,
            "range": "+/- 3.011",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 396.441,
            "range": "+/- 2.443",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12496.44,
            "range": "+/- 71.651",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 645.606,
            "range": "+/- 1.636",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3899.582,
            "range": "+/- 27.703",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 496.036,
            "range": "+/- 1.276",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1227.066,
            "range": "+/- 1.506",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 458.322,
            "range": "+/- 2.207",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13407.391,
            "range": "+/- 52.583",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2793.359,
            "range": "+/- 17.243",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 114.359,
            "range": "+/- 1.449",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 133.434,
            "range": "+/- 1.908",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1186.583,
            "range": "+/- 2.864",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1948.275,
            "range": "+/- 12.889",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 645.123,
            "range": "+/- 1.414",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 823.03,
            "range": "+/- 2.086",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1068.285,
            "range": "+/- 2.072",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12012.241,
            "range": "+/- 10.79",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 502.326,
            "range": "+/- 1.136",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3257.868,
            "range": "+/- 6.209",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 363.075,
            "range": "+/- 1.145",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75690.033,
            "range": "+/- 477.675",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11318.613,
            "range": "+/- 94.956",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 421.062,
            "range": "+/- 2.772",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 16.908,
            "range": "+/- 0.106",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 101.762,
            "range": "+/- 0.392",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.228,
            "range": "+/- 0.095",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1661.264,
            "range": "+/- 10.936",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 35.395,
            "range": "+/- 0.132",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2078.305,
            "range": "+/- 11.644",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15822.829,
            "range": "+/- 240.86",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 839.036,
            "range": "+/- 0.685",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747725.1,
            "range": "+/- 284.704",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11782.974,
            "range": "+/- 5.365",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 281.37,
            "range": "+/- 0.495",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3027.138,
            "range": "+/- 2.424",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 140.368,
            "range": "+/- 0.516",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46782.6,
            "range": "+/- 19.939",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2620.919,
            "range": "+/- 47.936",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2235981.348,
            "range": "+/- 6009.782",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 41765.008,
            "range": "+/- 723.334",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 891.768,
            "range": "+/- 2.383",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9630.846,
            "range": "+/- 129.376",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 326.917,
            "range": "+/- 1.145",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 143845.168,
            "range": "+/- 1393.146",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6896.055,
            "range": "+/- 67.198",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7767.208,
            "range": "+/- 45.761",
            "unit": "ns"
          }
        ]
      },
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
          "id": "cc7f78b8685986a9685c8aee82c0877930208b4d",
          "message": "ci: bump actions/download-artifact from 4 to 8",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/37/commits/cc7f78b8685986a9685c8aee82c0877930208b4d"
        },
        "date": 1788308064012,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 516.927,
            "range": "+/- 2.373",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3384.491,
            "range": "+/- 30.941",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 396.576,
            "range": "+/- 0.997",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1016.925,
            "range": "+/- 4.936",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 370.99,
            "range": "+/- 2.853",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12586.476,
            "range": "+/- 35.884",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 574.197,
            "range": "+/- 2.724",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6018.127,
            "range": "+/- 60.578",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 459.945,
            "range": "+/- 4.441",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1106.403,
            "range": "+/- 4.694",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 425.699,
            "range": "+/- 2.941",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 47099.582,
            "range": "+/- 150.474",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2471.019,
            "range": "+/- 12.974",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.78,
            "range": "+/- 0.466",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 114.917,
            "range": "+/- 0.723",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1071.592,
            "range": "+/- 5.162",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1708.828,
            "range": "+/- 2.586",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 629.641,
            "range": "+/- 5.453",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 799.826,
            "range": "+/- 17.495",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 945.497,
            "range": "+/- 2.881",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10677.277,
            "range": "+/- 9.215",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 444.759,
            "range": "+/- 0.819",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2906.069,
            "range": "+/- 4.858",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 322.048,
            "range": "+/- 1.581",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 77955.345,
            "range": "+/- 1449.124",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12573.846,
            "range": "+/- 151.036",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 357.051,
            "range": "+/- 0.938",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.534,
            "range": "+/- 0.091",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 95.53,
            "range": "+/- 0.497",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.739,
            "range": "+/- 0.1",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1406.013,
            "range": "+/- 2.815",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.158,
            "range": "+/- 0.218",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2100.628,
            "range": "+/- 28.345",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17247.339,
            "range": "+/- 245.383",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 737.267,
            "range": "+/- 1.269",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 664195.6,
            "range": "+/- 1604.653",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10426.715,
            "range": "+/- 5.288",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 247.581,
            "range": "+/- 0.466",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2681.527,
            "range": "+/- 3.091",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 126.975,
            "range": "+/- 0.534",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41586.607,
            "range": "+/- 33.41",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2276.237,
            "range": "+/- 13.332",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1986841.615,
            "range": "+/- 6347.758",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31591.145,
            "range": "+/- 152.731",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 796.25,
            "range": "+/- 3.111",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8141.219,
            "range": "+/- 27.981",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 297.405,
            "range": "+/- 1.582",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124504.272,
            "range": "+/- 346.724",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7566.595,
            "range": "+/- 84.231",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8368.764,
            "range": "+/- 69.379",
            "unit": "ns"
          }
        ]
      },
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
          "id": "3b9e6639a3079e70e8da160d6b26ef8bf8312c00",
          "message": "ci: bump hashicorp/setup-terraform from 3 to 4",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/39/commits/3b9e6639a3079e70e8da160d6b26ef8bf8312c00"
        },
        "date": 1788308147992,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 428.298,
            "range": "+/- 1.239",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2711.749,
            "range": "+/- 6.89",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 330.033,
            "range": "+/- 1.18",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 848.859,
            "range": "+/- 3.07",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 307.919,
            "range": "+/- 0.936",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 11627.45,
            "range": "+/- 32.665",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 507.205,
            "range": "+/- 2.153",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3387.69,
            "range": "+/- 17.543",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 388.854,
            "range": "+/- 1.177",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 981.616,
            "range": "+/- 4.265",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 367.575,
            "range": "+/- 1.185",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 10715.664,
            "range": "+/- 60.28",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2528.186,
            "range": "+/- 69.025",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 92.352,
            "range": "+/- 0.564",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 104.681,
            "range": "+/- 0.311",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1116.245,
            "range": "+/- 2.897",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1801.612,
            "range": "+/- 2.797",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 596.707,
            "range": "+/- 1.48",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 766.427,
            "range": "+/- 1.546",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 963.896,
            "range": "+/- 2.911",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11281.251,
            "range": "+/- 12.871",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 438.727,
            "range": "+/- 1.23",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3019.381,
            "range": "+/- 4.183",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 309.072,
            "range": "+/- 0.762",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73023.512,
            "range": "+/- 428.876",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12057.605,
            "range": "+/- 86.3",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 412.709,
            "range": "+/- 3.366",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 15.713,
            "range": "+/- 0.104",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 103.802,
            "range": "+/- 0.615",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.252,
            "range": "+/- 0.154",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1643.777,
            "range": "+/- 12.845",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 33.791,
            "range": "+/- 0.488",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2190.028,
            "range": "+/- 44.31",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16867.17,
            "range": "+/- 115.892",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 815.176,
            "range": "+/- 16.584",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 714130.122,
            "range": "+/- 885.722",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 12247.377,
            "range": "+/- 287.622",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 248.872,
            "range": "+/- 0.448",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2899.82,
            "range": "+/- 6.766",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 118.453,
            "range": "+/- 0.516",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 44824.428,
            "range": "+/- 160.725",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2065.322,
            "range": "+/- 17.787",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1802555.376,
            "range": "+/- 6924.071",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 28127.719,
            "range": "+/- 55.484",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 734.414,
            "range": "+/- 5.033",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7311.222,
            "range": "+/- 32.852",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 275.082,
            "range": "+/- 1.026",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 115286.074,
            "range": "+/- 934.609",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7178.771,
            "range": "+/- 46.007",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7919.883,
            "range": "+/- 24.679",
            "unit": "ns"
          }
        ]
      },
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
          "id": "38f486aac80732efd4837fcc418867e3879b1e74",
          "message": "ci: bump azure/setup-kubectl from 4 to 5",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/36/commits/38f486aac80732efd4837fcc418867e3879b1e74"
        },
        "date": 1788308182841,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 582.972,
            "range": "+/- 3.597",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3744.409,
            "range": "+/- 38.905",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 476.766,
            "range": "+/- 4.487",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1058.362,
            "range": "+/- 4.379",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 410.244,
            "range": "+/- 2.126",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12831.981,
            "range": "+/- 78.015",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 635.572,
            "range": "+/- 2.96",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 4107.545,
            "range": "+/- 47.452",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 504.211,
            "range": "+/- 3.451",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1231.859,
            "range": "+/- 10.38",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 464.928,
            "range": "+/- 1.977",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 14027.206,
            "range": "+/- 126.177",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2756.103,
            "range": "+/- 14.373",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 124.017,
            "range": "+/- 2.73",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 138.665,
            "range": "+/- 1.298",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1211.605,
            "range": "+/- 5.178",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1994.266,
            "range": "+/- 10.416",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 673.635,
            "range": "+/- 4.984",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 837.76,
            "range": "+/- 2.573",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1058.155,
            "range": "+/- 2.012",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12045.398,
            "range": "+/- 14.489",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 503.401,
            "range": "+/- 1.815",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3260.256,
            "range": "+/- 5.198",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 363.758,
            "range": "+/- 1.516",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 80177.221,
            "range": "+/- 1208.743",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12814.291,
            "range": "+/- 276.754",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 470.847,
            "range": "+/- 3.494",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.63,
            "range": "+/- 0.209",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 115.636,
            "range": "+/- 0.912",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.704,
            "range": "+/- 0.237",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1852.729,
            "range": "+/- 11.964",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 37.007,
            "range": "+/- 0.3",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2324.13,
            "range": "+/- 29.061",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 18691.882,
            "range": "+/- 756.708",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 843.187,
            "range": "+/- 0.915",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 751516.302,
            "range": "+/- 1522.368",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11922.915,
            "range": "+/- 29.523",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 279.425,
            "range": "+/- 0.393",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3060.188,
            "range": "+/- 5.8",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 144.626,
            "range": "+/- 0.799",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46852.136,
            "range": "+/- 34.565",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2524.487,
            "range": "+/- 6.392",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2319981.217,
            "range": "+/- 23778.012",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 37692.187,
            "range": "+/- 464.52",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 938.565,
            "range": "+/- 11.093",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9208.797,
            "range": "+/- 48.04",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 353.481,
            "range": "+/- 5.452",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 141104.756,
            "range": "+/- 938.706",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6937.378,
            "range": "+/- 70.157",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8164.177,
            "range": "+/- 135.143",
            "unit": "ns"
          }
        ]
      },
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
          "id": "c3cf460cb765ef4e578943da2840dff482a09e0b",
          "message": "ci: bump aws-actions/configure-aws-credentials from 4 to 5",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/40/commits/c3cf460cb765ef4e578943da2840dff482a09e0b"
        },
        "date": 1788308205071,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 541.154,
            "range": "+/- 3.336",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3372.298,
            "range": "+/- 14.271",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 398.669,
            "range": "+/- 0.555",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1041.477,
            "range": "+/- 9.596",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 373.372,
            "range": "+/- 1.472",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12642.196,
            "range": "+/- 71.108",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 658.014,
            "range": "+/- 3.648",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3845.953,
            "range": "+/- 25.548",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 487.174,
            "range": "+/- 2.273",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1221.994,
            "range": "+/- 11.862",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 440.123,
            "range": "+/- 1.481",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12272.874,
            "range": "+/- 50.082",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2697.673,
            "range": "+/- 16.882",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 106.248,
            "range": "+/- 2.068",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 114.79,
            "range": "+/- 0.296",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1208.765,
            "range": "+/- 1.328",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1942.064,
            "range": "+/- 6.92",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 675.965,
            "range": "+/- 4.846",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 872.862,
            "range": "+/- 5.004",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1057.532,
            "range": "+/- 1.164",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 13024.787,
            "range": "+/- 106.333",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 496.864,
            "range": "+/- 0.58",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3248.981,
            "range": "+/- 2.089",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 359.406,
            "range": "+/- 0.88",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 81425.818,
            "range": "+/- 922.168",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12817.206,
            "range": "+/- 301.813",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 396.114,
            "range": "+/- 1.527",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 18.706,
            "range": "+/- 0.219",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 99.536,
            "range": "+/- 0.261",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.877,
            "range": "+/- 0.105",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1616.193,
            "range": "+/- 6.741",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.337,
            "range": "+/- 0.116",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2488.96,
            "range": "+/- 27.269",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 20714.597,
            "range": "+/- 386.58",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 838.456,
            "range": "+/- 0.649",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 752415.818,
            "range": "+/- 1090.888",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11794.773,
            "range": "+/- 8.927",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 280.296,
            "range": "+/- 0.55",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3029.623,
            "range": "+/- 1.304",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 140.529,
            "range": "+/- 0.662",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46909.462,
            "range": "+/- 31.416",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2546.87,
            "range": "+/- 10.495",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2243169.273,
            "range": "+/- 8761.3",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 37207.842,
            "range": "+/- 574.269",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 906.069,
            "range": "+/- 6.759",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9116.315,
            "range": "+/- 37.471",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 334.13,
            "range": "+/- 2.037",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 143983.148,
            "range": "+/- 1109.593",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7537.39,
            "range": "+/- 101.02",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8406.807,
            "range": "+/- 145.402",
            "unit": "ns"
          }
        ]
      },
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
          "id": "561436fd4f67d4b8f80a06c072e7c79e341541c6",
          "message": "ci: bump azure/k8s-set-context from 4 to 5",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/38/commits/561436fd4f67d4b8f80a06c072e7c79e341541c6"
        },
        "date": 1788308714857,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 423.504,
            "range": "+/- 2.78",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2607.524,
            "range": "+/- 13.89",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 556.878,
            "range": "+/- 5.307",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 821.488,
            "range": "+/- 6.704",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 305.117,
            "range": "+/- 0.906",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 11144.962,
            "range": "+/- 34.624",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 455.962,
            "range": "+/- 2.643",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3140.001,
            "range": "+/- 9.586",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 360.043,
            "range": "+/- 1.243",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 914.237,
            "range": "+/- 8.085",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 342.174,
            "range": "+/- 1.609",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 10078.398,
            "range": "+/- 40.066",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2005.99,
            "range": "+/- 5.855",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 85.826,
            "range": "+/- 0.738",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 94.966,
            "range": "+/- 0.399",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1043.014,
            "range": "+/- 7.525",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1634.73,
            "range": "+/- 9.345",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 549.37,
            "range": "+/- 2.757",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 697.633,
            "range": "+/- 3.746",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 889.495,
            "range": "+/- 5.351",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10232.973,
            "range": "+/- 46.885",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 402.007,
            "range": "+/- 1.216",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2759.908,
            "range": "+/- 8.214",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 284.498,
            "range": "+/- 0.701",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 59401.145,
            "range": "+/- 447.552",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 10946.808,
            "range": "+/- 82.999",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 354.168,
            "range": "+/- 1.055",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 14.531,
            "range": "+/- 0.151",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 92.984,
            "range": "+/- 0.315",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 19.147,
            "range": "+/- 0.184",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1460.298,
            "range": "+/- 9.642",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 29.997,
            "range": "+/- 0.27",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1463.701,
            "range": "+/- 6.56",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 14058.936,
            "range": "+/- 91.47",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 701.946,
            "range": "+/- 1.91",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 653886.787,
            "range": "+/- 3469.223",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10107.868,
            "range": "+/- 22.953",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 231.07,
            "range": "+/- 1.084",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2584.139,
            "range": "+/- 7.068",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 109.882,
            "range": "+/- 0.181",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 40980.3,
            "range": "+/- 185.915",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1895.988,
            "range": "+/- 8.745",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1621343.554,
            "range": "+/- 7921.02",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 26581.278,
            "range": "+/- 120.29",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 670.424,
            "range": "+/- 2.782",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 6730.177,
            "range": "+/- 34.332",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 255.026,
            "range": "+/- 0.922",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 101067.825,
            "range": "+/- 371.66",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6219.871,
            "range": "+/- 12.49",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7020.26,
            "range": "+/- 38.675",
            "unit": "ns"
          }
        ]
      },
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
          "id": "3d7f01a4a3b7a33fcf297424098620e969a2548d",
          "message": "deps: update hashicorp/helm requirement from ~> 2.0 to ~> 3.2 in /deploy/terraform",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/52/commits/3d7f01a4a3b7a33fcf297424098620e969a2548d"
        },
        "date": 1788308723645,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 542.152,
            "range": "+/- 1.864",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3478.598,
            "range": "+/- 10.53",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 406.531,
            "range": "+/- 1.534",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1038.379,
            "range": "+/- 15.404",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 424.847,
            "range": "+/- 8.77",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12539.236,
            "range": "+/- 34.828",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 617.542,
            "range": "+/- 1.738",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3848.926,
            "range": "+/- 22.466",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 486.635,
            "range": "+/- 3.22",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1177.62,
            "range": "+/- 3.648",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 440.6,
            "range": "+/- 2.104",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13669.417,
            "range": "+/- 95.242",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2767.645,
            "range": "+/- 22.058",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 101.677,
            "range": "+/- 0.484",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 117.345,
            "range": "+/- 0.566",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1210.294,
            "range": "+/- 5.851",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1986.08,
            "range": "+/- 18.89",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 664.735,
            "range": "+/- 2.694",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 847.918,
            "range": "+/- 2.94",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1055.679,
            "range": "+/- 1.192",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11990.297,
            "range": "+/- 4.998",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 497.98,
            "range": "+/- 0.962",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3245.729,
            "range": "+/- 3.775",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 376.124,
            "range": "+/- 2.795",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 78803.61,
            "range": "+/- 839.821",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12190.849,
            "range": "+/- 236.045",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 408.209,
            "range": "+/- 2.281",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.658,
            "range": "+/- 0.168",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 102.876,
            "range": "+/- 0.374",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.161,
            "range": "+/- 0.18",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1702.014,
            "range": "+/- 14.559",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 35.344,
            "range": "+/- 0.132",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2166.819,
            "range": "+/- 33.646",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17722.104,
            "range": "+/- 348.95",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 842.712,
            "range": "+/- 1.279",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 749971.465,
            "range": "+/- 1071.608",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11801.892,
            "range": "+/- 13.403",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 281.249,
            "range": "+/- 0.471",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3031.94,
            "range": "+/- 2.006",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 141.647,
            "range": "+/- 0.402",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46896.391,
            "range": "+/- 37.483",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2788.065,
            "range": "+/- 42.685",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2264582.091,
            "range": "+/- 11931.158",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35941.016,
            "range": "+/- 238.755",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 895.557,
            "range": "+/- 1.925",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9514.549,
            "range": "+/- 82.266",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 330.581,
            "range": "+/- 1.311",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 159064.536,
            "range": "+/- 2134.945",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7215.845,
            "range": "+/- 88.245",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8020.381,
            "range": "+/- 113.767",
            "unit": "ns"
          }
        ]
      },
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
          "id": "ea980d4c5125f22c0ee66dfb65d040648154f30e",
          "message": "fix(ci): the baseline comparison failed builds on measurement noise",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/76/commits/ea980d4c5125f22c0ee66dfb65d040648154f30e"
        },
        "date": 1788309073232,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 513.3,
            "range": "+/- 2.438",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3400.045,
            "range": "+/- 12.557",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 391.267,
            "range": "+/- 1.632",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 997.388,
            "range": "+/- 3.871",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 356.282,
            "range": "+/- 1.108",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12828.723,
            "range": "+/- 44.848",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 583.637,
            "range": "+/- 4.2",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6140.536,
            "range": "+/- 23.437",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 454.445,
            "range": "+/- 1.944",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1131.685,
            "range": "+/- 3.975",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 421.908,
            "range": "+/- 1.821",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13537.998,
            "range": "+/- 83.069",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2574.057,
            "range": "+/- 26.119",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 112.386,
            "range": "+/- 2.275",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 125.788,
            "range": "+/- 1.631",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1092.42,
            "range": "+/- 2.878",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1793.936,
            "range": "+/- 5.419",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 582.371,
            "range": "+/- 1.21",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 749.851,
            "range": "+/- 1.387",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1037.678,
            "range": "+/- 18.052",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10655.875,
            "range": "+/- 19.819",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 444.868,
            "range": "+/- 2.021",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2899.267,
            "range": "+/- 2.82",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 317.054,
            "range": "+/- 0.766",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73401.209,
            "range": "+/- 778.782",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 14537.319,
            "range": "+/- 318.153",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 360.242,
            "range": "+/- 0.769",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.98,
            "range": "+/- 0.135",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 93.693,
            "range": "+/- 0.282",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.839,
            "range": "+/- 0.106",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1482.642,
            "range": "+/- 9.717",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.598,
            "range": "+/- 0.153",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1934.989,
            "range": "+/- 5.329",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 18153.314,
            "range": "+/- 296.129",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 733.134,
            "range": "+/- 0.661",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662327.473,
            "range": "+/- 241.582",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10433.499,
            "range": "+/- 6.391",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 250.029,
            "range": "+/- 0.807",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2690.697,
            "range": "+/- 4.456",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 128.305,
            "range": "+/- 0.65",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41548.428,
            "range": "+/- 93.85",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2258.362,
            "range": "+/- 10.409",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1983829.615,
            "range": "+/- 5870.073",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31484.821,
            "range": "+/- 93.507",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 797.646,
            "range": "+/- 3.173",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8158.915,
            "range": "+/- 44.44",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 297.97,
            "range": "+/- 1.781",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124019.088,
            "range": "+/- 211",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7430.232,
            "range": "+/- 81.005",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8506.688,
            "range": "+/- 114.159",
            "unit": "ns"
          }
        ]
      },
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
          "id": "70065f101d5e31d3de464a23bf8b01c88affc3ed",
          "message": "deps: update hashicorp/kubernetes requirement from ~> 2.0 to ~> 3.2 in /deploy/terraform",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/54/commits/70065f101d5e31d3de464a23bf8b01c88affc3ed"
        },
        "date": 1788364098576,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 409.874,
            "range": "+/- 0.54",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2694.167,
            "range": "+/- 8.373",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 317.444,
            "range": "+/- 0.774",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 774.64,
            "range": "+/- 2.447",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 287.841,
            "range": "+/- 0.562",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 9863.438,
            "range": "+/- 61.258",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 482.773,
            "range": "+/- 1.423",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 2938.234,
            "range": "+/- 4.998",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 375.29,
            "range": "+/- 0.418",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 921.817,
            "range": "+/- 1.774",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 346.797,
            "range": "+/- 0.545",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 10430.608,
            "range": "+/- 29.345",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2073.695,
            "range": "+/- 4.25",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 91.419,
            "range": "+/- 0.476",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 91.036,
            "range": "+/- 0.48",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 911.748,
            "range": "+/- 2.085",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1467.88,
            "range": "+/- 3.623",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 513.157,
            "range": "+/- 2.006",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 702.147,
            "range": "+/- 24.839",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 835.987,
            "range": "+/- 3.069",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9321.913,
            "range": "+/- 9.728",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 389.069,
            "range": "+/- 0.925",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2518.1,
            "range": "+/- 2.228",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 278.715,
            "range": "+/- 0.627",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 57451.149,
            "range": "+/- 139.469",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8787.668,
            "range": "+/- 60.949",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 306.596,
            "range": "+/- 0.87",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 13.149,
            "range": "+/- 0.142",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 83.019,
            "range": "+/- 0.272",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 17.043,
            "range": "+/- 0.063",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1227.191,
            "range": "+/- 3.05",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 26.874,
            "range": "+/- 0.048",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1555.15,
            "range": "+/- 4.514",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11796.365,
            "range": "+/- 33.091",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 649.564,
            "range": "+/- 0.375",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 578831.331,
            "range": "+/- 250.864",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9117.587,
            "range": "+/- 2.057",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 216.174,
            "range": "+/- 0.39",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2344.981,
            "range": "+/- 2.316",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 107.544,
            "range": "+/- 0.158",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 36205.817,
            "range": "+/- 12.282",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2021.958,
            "range": "+/- 34.187",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1746331.655,
            "range": "+/- 10241.572",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 27446.492,
            "range": "+/- 108.49",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 688.68,
            "range": "+/- 1.159",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7203.958,
            "range": "+/- 85.806",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 253.571,
            "range": "+/- 0.704",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 108733.218,
            "range": "+/- 375.445",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5311.102,
            "range": "+/- 58.811",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5927.8,
            "range": "+/- 31.531",
            "unit": "ns"
          }
        ]
      },
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
          "id": "0c3ff7478f6e9abe1992e2da3716d45ac0ef30f9",
          "message": "deps(deps): bump wasmtime from 24.0.12 to 48.0.1",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/63/commits/0c3ff7478f6e9abe1992e2da3716d45ac0ef30f9"
        },
        "date": 1788364171922,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 409.351,
            "range": "+/- 1.556",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2602.413,
            "range": "+/- 14.314",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 326.177,
            "range": "+/- 0.588",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 777.306,
            "range": "+/- 2.293",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 288.05,
            "range": "+/- 0.675",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 9828.048,
            "range": "+/- 31.696",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 477.428,
            "range": "+/- 1.066",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 2970.399,
            "range": "+/- 8.017",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 368.315,
            "range": "+/- 0.634",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 909.083,
            "range": "+/- 4.189",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 337.131,
            "range": "+/- 1.248",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 10441.21,
            "range": "+/- 41.337",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2044.684,
            "range": "+/- 7.56",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 79.094,
            "range": "+/- 0.428",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 92.576,
            "range": "+/- 0.964",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 927.664,
            "range": "+/- 2.37",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1507.457,
            "range": "+/- 1.447",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 497.993,
            "range": "+/- 1.532",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 643.641,
            "range": "+/- 2.346",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 816.485,
            "range": "+/- 0.782",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9306.357,
            "range": "+/- 17.187",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 404.635,
            "range": "+/- 4.743",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2512.602,
            "range": "+/- 1.765",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 283.202,
            "range": "+/- 1.047",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 58105.124,
            "range": "+/- 154.635",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8727.318,
            "range": "+/- 71.458",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 304.102,
            "range": "+/- 0.709",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 12.936,
            "range": "+/- 0.048",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 77.973,
            "range": "+/- 0.108",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 17.029,
            "range": "+/- 0.059",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1206.928,
            "range": "+/- 1.411",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 27.088,
            "range": "+/- 0.08",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1687.563,
            "range": "+/- 6.079",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 11943.462,
            "range": "+/- 52.563",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 667.88,
            "range": "+/- 0.902",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 579011.205,
            "range": "+/- 325.682",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9133.965,
            "range": "+/- 5.309",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 230.94,
            "range": "+/- 0.205",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2357.887,
            "range": "+/- 1.694",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 128.187,
            "range": "+/- 0.282",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 36292.052,
            "range": "+/- 35.918",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 1962.972,
            "range": "+/- 7.893",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1916682.057,
            "range": "+/- 26296.96",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 27604.223,
            "range": "+/- 190.052",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 696.398,
            "range": "+/- 2.59",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 7028.472,
            "range": "+/- 33.178",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 253.228,
            "range": "+/- 0.623",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124100.815,
            "range": "+/- 1762.517",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5206.023,
            "range": "+/- 20.434",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 5828.147,
            "range": "+/- 17.836",
            "unit": "ns"
          }
        ]
      },
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
          "id": "058afc575248c0f41c9a9cf6a066ad5e2df6b74f",
          "message": "deps(deps): bump wgpu from 22.1.0 to 30.0.1",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/46/commits/058afc575248c0f41c9a9cf6a066ad5e2df6b74f"
        },
        "date": 1788364262777,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 556.407,
            "range": "+/- 1.373",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3358.453,
            "range": "+/- 10.915",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 425.07,
            "range": "+/- 1.882",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1034.018,
            "range": "+/- 8.003",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 390.775,
            "range": "+/- 1.66",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12664.131,
            "range": "+/- 51.602",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 631.998,
            "range": "+/- 1.733",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3805.186,
            "range": "+/- 14.529",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 489.948,
            "range": "+/- 2.288",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1194.162,
            "range": "+/- 7.563",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 448.752,
            "range": "+/- 1.495",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 11944.737,
            "range": "+/- 61.293",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2781.878,
            "range": "+/- 12.715",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.844,
            "range": "+/- 0.26",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 115.647,
            "range": "+/- 0.614",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1206.578,
            "range": "+/- 6.659",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1907.492,
            "range": "+/- 5.491",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 640.857,
            "range": "+/- 1.243",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 819.633,
            "range": "+/- 1.079",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1071.028,
            "range": "+/- 1.995",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12245.079,
            "range": "+/- 110.751",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 512.916,
            "range": "+/- 2.098",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3269.268,
            "range": "+/- 3.965",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 371.978,
            "range": "+/- 1.665",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 76188.188,
            "range": "+/- 352.645",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11099.015,
            "range": "+/- 45.473",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 399.808,
            "range": "+/- 0.902",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 16.859,
            "range": "+/- 0.092",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 103.639,
            "range": "+/- 0.254",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.183,
            "range": "+/- 0.103",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1595.752,
            "range": "+/- 2.524",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.528,
            "range": "+/- 0.087",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2149.261,
            "range": "+/- 15.133",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16248.162,
            "range": "+/- 175.67",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 841.917,
            "range": "+/- 0.322",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 754824.735,
            "range": "+/- 1189.645",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11816.497,
            "range": "+/- 8.282",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 284.228,
            "range": "+/- 0.722",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3035.095,
            "range": "+/- 2.668",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 144.814,
            "range": "+/- 0.318",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46885.476,
            "range": "+/- 23.078",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2535.373,
            "range": "+/- 8.677",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2422537.955,
            "range": "+/- 25633.514",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35257.329,
            "range": "+/- 119.262",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 915.61,
            "range": "+/- 10.932",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9261.308,
            "range": "+/- 36.399",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 334.733,
            "range": "+/- 1.975",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 143297.067,
            "range": "+/- 928.947",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6874.814,
            "range": "+/- 43.358",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7752.163,
            "range": "+/- 68.093",
            "unit": "ns"
          }
        ]
      },
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
          "id": "07924eb74cb944b63c5152804e5fd42052bb22a7",
          "message": "deps(deps): bump spin from 0.9.9 to 0.12.3",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/64/commits/07924eb74cb944b63c5152804e5fd42052bb22a7"
        },
        "date": 1788364443127,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 637.996,
            "range": "+/- 1.269",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3580.364,
            "range": "+/- 17.679",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 521.734,
            "range": "+/- 3.151",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1156.816,
            "range": "+/- 8.069",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 474.118,
            "range": "+/- 1.915",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12518.656,
            "range": "+/- 34.493",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 672.175,
            "range": "+/- 2.181",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 4059.58,
            "range": "+/- 33.627",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 542.967,
            "range": "+/- 4.334",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1227.624,
            "range": "+/- 3.999",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 497.78,
            "range": "+/- 1.036",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13712.832,
            "range": "+/- 70.037",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2847.199,
            "range": "+/- 21.235",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.867,
            "range": "+/- 0.356",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 115.905,
            "range": "+/- 0.552",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1252.624,
            "range": "+/- 1.998",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1963.168,
            "range": "+/- 4.153",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 698.037,
            "range": "+/- 0.943",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 880.435,
            "range": "+/- 1.075",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1055.923,
            "range": "+/- 0.961",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11979.425,
            "range": "+/- 6.16",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 503.16,
            "range": "+/- 1.067",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3243.077,
            "range": "+/- 1.811",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 361.4,
            "range": "+/- 0.922",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 93769.042,
            "range": "+/- 1235.922",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11220.444,
            "range": "+/- 69.629",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 395.985,
            "range": "+/- 0.892",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.09,
            "range": "+/- 0.141",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 103.103,
            "range": "+/- 0.338",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.946,
            "range": "+/- 0.109",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1566.421,
            "range": "+/- 2.008",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.762,
            "range": "+/- 0.167",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2365.948,
            "range": "+/- 29.355",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16197.186,
            "range": "+/- 180.024",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 841.621,
            "range": "+/- 0.95",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 750269.151,
            "range": "+/- 481.423",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11831.518,
            "range": "+/- 14.311",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 290.574,
            "range": "+/- 0.245",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3074.275,
            "range": "+/- 5.657",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 150.952,
            "range": "+/- 0.357",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 48210.22,
            "range": "+/- 248.911",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2600.507,
            "range": "+/- 21.076",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2281666.261,
            "range": "+/- 16548.123",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35315.685,
            "range": "+/- 150.457",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 949.235,
            "range": "+/- 20.801",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9058.773,
            "range": "+/- 19.997",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 333.51,
            "range": "+/- 3.554",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140309.744,
            "range": "+/- 1090.207",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7476.995,
            "range": "+/- 118.792",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8157.355,
            "range": "+/- 116.894",
            "unit": "ns"
          }
        ]
      },
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
          "id": "4000913c9cf31d63049a94ababa171bff0ffcb72",
          "message": "deps(deps): bump windows-sys from 0.59.0 to 0.61.2",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/50/commits/4000913c9cf31d63049a94ababa171bff0ffcb72"
        },
        "date": 1788364553057,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 534.809,
            "range": "+/- 2.736",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3383.692,
            "range": "+/- 7.804",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 396.306,
            "range": "+/- 0.838",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1016.359,
            "range": "+/- 2.807",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 372.633,
            "range": "+/- 2.56",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 14913.246,
            "range": "+/- 76.048",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 614.792,
            "range": "+/- 1.648",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3788.733,
            "range": "+/- 14.358",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 475.03,
            "range": "+/- 1.623",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1181.241,
            "range": "+/- 3.327",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 437.738,
            "range": "+/- 0.596",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 17142.986,
            "range": "+/- 44.584",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2823.09,
            "range": "+/- 10.208",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 99.671,
            "range": "+/- 0.371",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 116.249,
            "range": "+/- 0.493",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1257.187,
            "range": "+/- 5.359",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 2048.233,
            "range": "+/- 14.443",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 668.481,
            "range": "+/- 1.942",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 861.16,
            "range": "+/- 2.612",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1056.551,
            "range": "+/- 0.754",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12006.303,
            "range": "+/- 8.443",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 499.303,
            "range": "+/- 0.382",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3243.232,
            "range": "+/- 1.608",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 363.162,
            "range": "+/- 0.939",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 393.952,
            "range": "+/- 1.408",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 18.703,
            "range": "+/- 0.183",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 103.334,
            "range": "+/- 0.513",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.997,
            "range": "+/- 0.238",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1573.808,
            "range": "+/- 3.737",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 36.412,
            "range": "+/- 0.238",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 841.699,
            "range": "+/- 1.242",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747482.449,
            "range": "+/- 403.216",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11809.61,
            "range": "+/- 6.247",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 280.844,
            "range": "+/- 0.549",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3030.015,
            "range": "+/- 2.001",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 140.481,
            "range": "+/- 0.393",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46774.364,
            "range": "+/- 22.734",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2706.232,
            "range": "+/- 50.111",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2229316.087,
            "range": "+/- 4555.331",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35191.829,
            "range": "+/- 106.918",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 894.251,
            "range": "+/- 3.104",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9106.068,
            "range": "+/- 52.117",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 328.082,
            "range": "+/- 1.203",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140253.44,
            "range": "+/- 694.66",
            "unit": "ns"
          }
        ]
      },
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
          "id": "62969029e16efd2913d1795dcbf0dc3f8ca7ee04",
          "message": "deps(deps): bump toml from 0.8.23 to 1.1.4+spec-1.1.0",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/68/commits/62969029e16efd2913d1795dcbf0dc3f8ca7ee04"
        },
        "date": 1788364865653,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 576.648,
            "range": "+/- 2.327",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3446.705,
            "range": "+/- 11.585",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 448.241,
            "range": "+/- 1.264",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1064.105,
            "range": "+/- 5.929",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 415.487,
            "range": "+/- 0.932",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12761.221,
            "range": "+/- 76.436",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 651.626,
            "range": "+/- 2.151",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3986.622,
            "range": "+/- 13.757",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 513.229,
            "range": "+/- 1.402",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1208.807,
            "range": "+/- 4.715",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 465.751,
            "range": "+/- 0.833",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13321.321,
            "range": "+/- 62.109",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2750.827,
            "range": "+/- 14.264",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 101.312,
            "range": "+/- 0.174",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 120.479,
            "range": "+/- 0.656",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1265.182,
            "range": "+/- 8.076",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1997.406,
            "range": "+/- 9.16",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 693.945,
            "range": "+/- 2.47",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 898.781,
            "range": "+/- 4.553",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1062.976,
            "range": "+/- 1.598",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12034.352,
            "range": "+/- 13.268",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 501.038,
            "range": "+/- 0.907",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3244.041,
            "range": "+/- 1.333",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 363.812,
            "range": "+/- 0.488",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75115.626,
            "range": "+/- 276.895",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11075.453,
            "range": "+/- 59.487",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 401.132,
            "range": "+/- 0.762",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.98,
            "range": "+/- 0.163",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 104.034,
            "range": "+/- 0.411",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.832,
            "range": "+/- 0.113",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1604.015,
            "range": "+/- 2.744",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 36.003,
            "range": "+/- 0.154",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1999.889,
            "range": "+/- 8.758",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15141.492,
            "range": "+/- 81.182",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 865.373,
            "range": "+/- 0.646",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747080.839,
            "range": "+/- 242.719",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11784.396,
            "range": "+/- 4.7",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 302.63,
            "range": "+/- 0.338",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3044.074,
            "range": "+/- 2.12",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 165.663,
            "range": "+/- 0.595",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46740.649,
            "range": "+/- 14.305",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2523.498,
            "range": "+/- 6.65",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2255197.909,
            "range": "+/- 7601.238",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35126.468,
            "range": "+/- 84.598",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 896.028,
            "range": "+/- 3.217",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9086.071,
            "range": "+/- 35.482",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 328.705,
            "range": "+/- 1.423",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140931.443,
            "range": "+/- 612.964",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6711.175,
            "range": "+/- 26.824",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7439.721,
            "range": "+/- 32.702",
            "unit": "ns"
          }
        ]
      },
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
          "id": "918901d909840a9e9985bb3c55c8f34841980ae6",
          "message": "deps: wgpu 22 -> 24, with the three source changes it needs",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/77/commits/918901d909840a9e9985bb3c55c8f34841980ae6"
        },
        "date": 1788365483465,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 537.679,
            "range": "+/- 2.842",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3389.953,
            "range": "+/- 19.075",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 406.135,
            "range": "+/- 0.896",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1019.408,
            "range": "+/- 4.983",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 383.011,
            "range": "+/- 0.71",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12330.726,
            "range": "+/- 47.094",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 619.912,
            "range": "+/- 1.749",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3865.262,
            "range": "+/- 11.992",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 481.664,
            "range": "+/- 1.76",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1183.113,
            "range": "+/- 4.199",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 444.508,
            "range": "+/- 3.238",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 11618.681,
            "range": "+/- 68.757",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2676.092,
            "range": "+/- 4.328",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.267,
            "range": "+/- 0.315",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 118.494,
            "range": "+/- 0.349",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1187.448,
            "range": "+/- 3.577",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1896.465,
            "range": "+/- 2.085",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 646.509,
            "range": "+/- 1.355",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 824.36,
            "range": "+/- 1.535",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1056.47,
            "range": "+/- 0.717",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 11990.5,
            "range": "+/- 5.558",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 503.461,
            "range": "+/- 1.547",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3243.644,
            "range": "+/- 1.628",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 361.433,
            "range": "+/- 0.48",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75518.016,
            "range": "+/- 310.899",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11257.918,
            "range": "+/- 94.272",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 393.528,
            "range": "+/- 1.242",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 16.498,
            "range": "+/- 0.021",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 101.365,
            "range": "+/- 0.202",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.881,
            "range": "+/- 0.06",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1575.092,
            "range": "+/- 3.058",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.405,
            "range": "+/- 0.118",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2039.967,
            "range": "+/- 6.913",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15724.35,
            "range": "+/- 107.512",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 842.805,
            "range": "+/- 1.249",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747664.554,
            "range": "+/- 403.477",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11774.658,
            "range": "+/- 8.442",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 280.78,
            "range": "+/- 0.526",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3027.413,
            "range": "+/- 2.992",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 139.715,
            "range": "+/- 0.177",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46928.19,
            "range": "+/- 75.961",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2513.536,
            "range": "+/- 2.748",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2229392.304,
            "range": "+/- 5440.591",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35062.65,
            "range": "+/- 69.95",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 887.402,
            "range": "+/- 1.339",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9231.245,
            "range": "+/- 119.804",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 327.783,
            "range": "+/- 1.164",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 144093.736,
            "range": "+/- 2345.378",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6734.873,
            "range": "+/- 26.522",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7561.275,
            "range": "+/- 33.201",
            "unit": "ns"
          }
        ]
      },
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
          "id": "94d51ef24d67028912fcaf44a68b2596f7061a1d",
          "message": "deps: wgpu 22 -> 30, with the source changes it needs",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/79/commits/94d51ef24d67028912fcaf44a68b2596f7061a1d"
        },
        "date": 1788366743431,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 519.677,
            "range": "+/- 5.957",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3470.343,
            "range": "+/- 35.387",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 386.149,
            "range": "+/- 2.873",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1028.258,
            "range": "+/- 12.522",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 343.836,
            "range": "+/- 2.11",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 21119.91,
            "range": "+/- 267.215",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 588.815,
            "range": "+/- 5.311",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6740.506,
            "range": "+/- 82.259",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 453.041,
            "range": "+/- 3.144",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1181.528,
            "range": "+/- 9.604",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 428.052,
            "range": "+/- 4.286",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 11974.686,
            "range": "+/- 163.507",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2518.226,
            "range": "+/- 17.5",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 103.69,
            "range": "+/- 1.155",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 118.261,
            "range": "+/- 1.016",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1093.229,
            "range": "+/- 4.477",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1787.371,
            "range": "+/- 7.965",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 601.903,
            "range": "+/- 4.915",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 758.35,
            "range": "+/- 3.283",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 936.488,
            "range": "+/- 2.038",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10638.693,
            "range": "+/- 8.717",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 443.446,
            "range": "+/- 1.26",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2880.516,
            "range": "+/- 3.711",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 323.868,
            "range": "+/- 1.396",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 71360.858,
            "range": "+/- 243.564",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12508.361,
            "range": "+/- 76.371",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 357.414,
            "range": "+/- 2.102",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 18.315,
            "range": "+/- 0.176",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 94.46,
            "range": "+/- 0.391",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.586,
            "range": "+/- 0.263",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 3444.632,
            "range": "+/- 564.746",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.343,
            "range": "+/- 0.176",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1929.876,
            "range": "+/- 9.866",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16419.995,
            "range": "+/- 60.165",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 817.707,
            "range": "+/- 5.008",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 663020.456,
            "range": "+/- 884.099",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11889.592,
            "range": "+/- 54.796",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 285.635,
            "range": "+/- 2.443",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3072.703,
            "range": "+/- 21.178",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 137.592,
            "range": "+/- 1.033",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41425.715,
            "range": "+/- 19.419",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2348.889,
            "range": "+/- 29.699",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2093740.08,
            "range": "+/- 25549.837",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 33214.682,
            "range": "+/- 408.743",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 879.304,
            "range": "+/- 19.104",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8313.121,
            "range": "+/- 73.534",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 309.484,
            "range": "+/- 4.319",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 128641.789,
            "range": "+/- 1283.397",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7308.245,
            "range": "+/- 44.453",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8106.835,
            "range": "+/- 36.724",
            "unit": "ns"
          }
        ]
      },
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
          "id": "08a48bcbf08a0de66634ce79467950b04dd3f4fe",
          "message": "chore: remove wasmtime, and the WASM claims that had no code behind them",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/80/commits/08a48bcbf08a0de66634ce79467950b04dd3f4fe"
        },
        "date": 1788367421108,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 518.453,
            "range": "+/- 4.061",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 9130.654,
            "range": "+/- 87.829",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 389.692,
            "range": "+/- 2.5",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1021.174,
            "range": "+/- 8.887",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 353,
            "range": "+/- 2.353",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 40499.601,
            "range": "+/- 447.809",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 586.241,
            "range": "+/- 1.649",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6285.047,
            "range": "+/- 64.66",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 481.878,
            "range": "+/- 6.212",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1143.755,
            "range": "+/- 10.105",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 429.979,
            "range": "+/- 4.224",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 46282.688,
            "range": "+/- 472.261",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2552.184,
            "range": "+/- 6.946",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 108.024,
            "range": "+/- 0.582",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 116.039,
            "range": "+/- 0.635",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1081.529,
            "range": "+/- 4.881",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1744.794,
            "range": "+/- 8.877",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 591.321,
            "range": "+/- 3.331",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 761.524,
            "range": "+/- 4.503",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 942.846,
            "range": "+/- 2.01",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10654.003,
            "range": "+/- 11.648",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 450.614,
            "range": "+/- 1.846",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2890.465,
            "range": "+/- 3.63",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 327.081,
            "range": "+/- 1.25",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73166.176,
            "range": "+/- 413.357",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12472.585,
            "range": "+/- 92.306",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 354.397,
            "range": "+/- 1.41",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 18.715,
            "range": "+/- 0.229",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 94.113,
            "range": "+/- 0.667",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.676,
            "range": "+/- 0.219",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1395.185,
            "range": "+/- 8.012",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 44.325,
            "range": "+/- 8.174",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1981.794,
            "range": "+/- 13.672",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16364.581,
            "range": "+/- 106.943",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 790.177,
            "range": "+/- 6.232",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 662258.166,
            "range": "+/- 453.642",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10440.688,
            "range": "+/- 12.273",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 288.006,
            "range": "+/- 3.707",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2685.767,
            "range": "+/- 2.614",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 151.107,
            "range": "+/- 1.232",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41449.12,
            "range": "+/- 40.993",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2314.626,
            "range": "+/- 25.233",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2063148.92,
            "range": "+/- 21523.315",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 32414.507,
            "range": "+/- 290.447",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 844.275,
            "range": "+/- 11.844",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8546.866,
            "range": "+/- 121.436",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 707.57,
            "range": "+/- 85.371",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 127084.327,
            "range": "+/- 764.972",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7252.582,
            "range": "+/- 48.679",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8144.405,
            "range": "+/- 50.254",
            "unit": "ns"
          }
        ]
      },
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
          "id": "176c2fd5c093dc2932734cad1b26959e37725810",
          "message": "deps: windows-sys 0.59 -> 0.61",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/78/commits/176c2fd5c093dc2932734cad1b26959e37725810"
        },
        "date": 1788368112242,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 501.522,
            "range": "+/- 2.031",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3350.889,
            "range": "+/- 13.201",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 387.127,
            "range": "+/- 1.18",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 987.297,
            "range": "+/- 2.066",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 353.662,
            "range": "+/- 0.948",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 34237.045,
            "range": "+/- 519.87",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 582.893,
            "range": "+/- 8.177",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 5101.239,
            "range": "+/- 19.963",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 444.062,
            "range": "+/- 0.872",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1148.803,
            "range": "+/- 9.994",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 401.9,
            "range": "+/- 1.761",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12939.227,
            "range": "+/- 42.594",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2465.164,
            "range": "+/- 13.632",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 103.101,
            "range": "+/- 0.529",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 117.837,
            "range": "+/- 0.535",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1124.857,
            "range": "+/- 6.67",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1800.061,
            "range": "+/- 16.216",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 611.909,
            "range": "+/- 4.225",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 804.832,
            "range": "+/- 7.78",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 931.042,
            "range": "+/- 0.89",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10716.251,
            "range": "+/- 15.056",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 441.991,
            "range": "+/- 1.212",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2895.078,
            "range": "+/- 3.239",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 330.184,
            "range": "+/- 1.948",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73741.235,
            "range": "+/- 285.588",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12674.243,
            "range": "+/- 48.733",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 353.99,
            "range": "+/- 0.766",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.686,
            "range": "+/- 0.138",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 93.765,
            "range": "+/- 0.542",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.671,
            "range": "+/- 0.049",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1397.459,
            "range": "+/- 1.476",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 32.394,
            "range": "+/- 0.121",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1943.587,
            "range": "+/- 7.463",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 17057.108,
            "range": "+/- 124.974",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 738.15,
            "range": "+/- 0.546",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 663085.781,
            "range": "+/- 363.639",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10420.392,
            "range": "+/- 4.394",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 247.937,
            "range": "+/- 0.377",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2681.476,
            "range": "+/- 1.936",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 126.441,
            "range": "+/- 0.368",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41552.892,
            "range": "+/- 85.192",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2356.216,
            "range": "+/- 53.723",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2192565.957,
            "range": "+/- 26740.452",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 36047.216,
            "range": "+/- 579.974",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 799.669,
            "range": "+/- 3.499",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8263.183,
            "range": "+/- 46.089",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 297.517,
            "range": "+/- 1.478",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 137543.075,
            "range": "+/- 1831.998",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7479.698,
            "range": "+/- 38.226",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8374.179,
            "range": "+/- 53.063",
            "unit": "ns"
          }
        ]
      },
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
          "id": "880970c8857e69c079306b3f4b0a5af5a606c451",
          "message": "chore: remove wasmtime, and the WASM claims that had no code behind them",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/80/commits/880970c8857e69c079306b3f4b0a5af5a606c451"
        },
        "date": 1788369686123,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 539.015,
            "range": "+/- 1.95",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3525.958,
            "range": "+/- 33.723",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 430.839,
            "range": "+/- 3.775",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1015.86,
            "range": "+/- 1.754",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 389.903,
            "range": "+/- 3.206",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 21324.638,
            "range": "+/- 147.34",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 641.281,
            "range": "+/- 3.232",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3814.42,
            "range": "+/- 16.105",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 493.971,
            "range": "+/- 2.866",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1231.148,
            "range": "+/- 11.025",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 442.665,
            "range": "+/- 1.972",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 11860.575,
            "range": "+/- 73.969",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2707.133,
            "range": "+/- 17.586",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 103.659,
            "range": "+/- 0.893",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 125.094,
            "range": "+/- 1.431",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1213.969,
            "range": "+/- 5.445",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1934.834,
            "range": "+/- 4.333",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 650.922,
            "range": "+/- 2.321",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 836.967,
            "range": "+/- 3.346",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1068.046,
            "range": "+/- 2.33",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12018.738,
            "range": "+/- 8.913",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 503.079,
            "range": "+/- 0.647",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 7533.478,
            "range": "+/- 1226.085",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 365.767,
            "range": "+/- 0.318",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75199.237,
            "range": "+/- 310.487",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11093.256,
            "range": "+/- 45.738",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 396.029,
            "range": "+/- 1.194",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.721,
            "range": "+/- 0.164",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 104.289,
            "range": "+/- 0.837",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.877,
            "range": "+/- 0.133",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1586.239,
            "range": "+/- 7.727",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 36.511,
            "range": "+/- 0.279",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2195.014,
            "range": "+/- 5.825",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15274.589,
            "range": "+/- 64.715",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 844.543,
            "range": "+/- 1.429",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 752738.176,
            "range": "+/- 1261.024",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11812.084,
            "range": "+/- 13.398",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 282.908,
            "range": "+/- 0.647",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3032.419,
            "range": "+/- 2.221",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 141.5,
            "range": "+/- 0.347",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46846.044,
            "range": "+/- 24.018",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 3127.579,
            "range": "+/- 33.142",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2284472.636,
            "range": "+/- 13959.088",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 41996.066,
            "range": "+/- 506.326",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 1111.214,
            "range": "+/- 11.24",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 11446.213,
            "range": "+/- 115.077",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 359.922,
            "range": "+/- 5.414",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 143773.253,
            "range": "+/- 1613.216",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6733.644,
            "range": "+/- 25.335",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7599.465,
            "range": "+/- 40.387",
            "unit": "ns"
          }
        ]
      },
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
          "id": "05960dd758df48d4f42e8f5b3ca4428a7335e525",
          "message": "deps: wgpu 22 -> 30, with the source changes it needs",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/79/commits/05960dd758df48d4f42e8f5b3ca4428a7335e525"
        },
        "date": 1788369711933,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 523.862,
            "range": "+/- 3.062",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3365.758,
            "range": "+/- 20.191",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 399.917,
            "range": "+/- 1.111",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1018.182,
            "range": "+/- 2.864",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 365.238,
            "range": "+/- 1.569",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12649.322,
            "range": "+/- 79.699",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 589.096,
            "range": "+/- 2.873",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 6274.289,
            "range": "+/- 31.78",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 454.598,
            "range": "+/- 1.174",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1139.151,
            "range": "+/- 4.559",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 423.126,
            "range": "+/- 2.662",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12846.816,
            "range": "+/- 42.669",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2446.593,
            "range": "+/- 9.532",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 100.733,
            "range": "+/- 0.491",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 115.275,
            "range": "+/- 1.074",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1147.662,
            "range": "+/- 6.427",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1920.542,
            "range": "+/- 25.056",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 586.833,
            "range": "+/- 2.245",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 765.31,
            "range": "+/- 3.092",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 964.827,
            "range": "+/- 3.792",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 10635.441,
            "range": "+/- 9.742",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 451.481,
            "range": "+/- 1.82",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2907.271,
            "range": "+/- 14.638",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 323,
            "range": "+/- 1.225",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73423.74,
            "range": "+/- 338.651",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 12431.371,
            "range": "+/- 60.248",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 354.33,
            "range": "+/- 0.624",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 19.427,
            "range": "+/- 0.257",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 96.267,
            "range": "+/- 1.159",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.738,
            "range": "+/- 0.364",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1403.426,
            "range": "+/- 2.607",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 31.946,
            "range": "+/- 0.185",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1930.662,
            "range": "+/- 10.932",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 16565.43,
            "range": "+/- 60.206",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 736.683,
            "range": "+/- 2.053",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 661418.731,
            "range": "+/- 416.047",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 10444.177,
            "range": "+/- 6.476",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 249.155,
            "range": "+/- 0.435",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2687.443,
            "range": "+/- 1.679",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 127.339,
            "range": "+/- 0.264",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 41412.65,
            "range": "+/- 33.65",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2271.884,
            "range": "+/- 15.074",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2023202.346,
            "range": "+/- 14028.258",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31649.757,
            "range": "+/- 283.128",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 806.237,
            "range": "+/- 5.641",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8106.226,
            "range": "+/- 43.916",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 295.751,
            "range": "+/- 1.064",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 124609.39,
            "range": "+/- 585.364",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7248.047,
            "range": "+/- 28.441",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 8233.504,
            "range": "+/- 55.542",
            "unit": "ns"
          }
        ]
      },
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
          "id": "fd8d9f4aa983e1a797594ef9bb85ee6cc02577b6",
          "message": "docs: name the legal entity as NERVOSYS, LLC",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/81/commits/fd8d9f4aa983e1a797594ef9bb85ee6cc02577b6"
        },
        "date": 1788371044563,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 529.344,
            "range": "+/- 1.991",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3286.833,
            "range": "+/- 10.269",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 399.927,
            "range": "+/- 0.917",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1019.325,
            "range": "+/- 4.623",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 369.423,
            "range": "+/- 0.917",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12366.578,
            "range": "+/- 26.921",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 615.496,
            "range": "+/- 1.71",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3814.922,
            "range": "+/- 16.013",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 474.206,
            "range": "+/- 1.593",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1176.108,
            "range": "+/- 2.568",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 440.239,
            "range": "+/- 1.549",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 12065.334,
            "range": "+/- 101.369",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2671.436,
            "range": "+/- 8.326",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 102.239,
            "range": "+/- 0.683",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 117.814,
            "range": "+/- 0.603",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1239.751,
            "range": "+/- 16.958",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1938.052,
            "range": "+/- 5.721",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 652.212,
            "range": "+/- 3.209",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 826.936,
            "range": "+/- 2.132",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1066.042,
            "range": "+/- 2.444",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12083.55,
            "range": "+/- 11.552",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 512.819,
            "range": "+/- 2.289",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3280.395,
            "range": "+/- 10.343",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 364.673,
            "range": "+/- 1.253",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75889.96,
            "range": "+/- 285.82",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11701.366,
            "range": "+/- 91.15",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 396.291,
            "range": "+/- 0.822",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.314,
            "range": "+/- 0.053",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 104.365,
            "range": "+/- 0.36",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 22.155,
            "range": "+/- 0.126",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1580.014,
            "range": "+/- 4.126",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.809,
            "range": "+/- 0.088",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2230.149,
            "range": "+/- 4.539",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15523.933,
            "range": "+/- 100.336",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 908.866,
            "range": "+/- 9.252",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 748637.148,
            "range": "+/- 440.985",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11787.958,
            "range": "+/- 7.076",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 284.402,
            "range": "+/- 0.826",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3028.125,
            "range": "+/- 3.001",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 144.336,
            "range": "+/- 1.756",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46681.049,
            "range": "+/- 14.878",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2518.843,
            "range": "+/- 6.221",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2326020.609,
            "range": "+/- 23946.585",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35096.885,
            "range": "+/- 71.541",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 893.726,
            "range": "+/- 2.506",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9083.219,
            "range": "+/- 43.039",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 329.83,
            "range": "+/- 1.187",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 139405.231,
            "range": "+/- 213.623",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6862.167,
            "range": "+/- 42.189",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7531.444,
            "range": "+/- 33.751",
            "unit": "ns"
          }
        ]
      },
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
          "id": "f7731559805167da2bd7c5d4ef2ee0da9afeaa9c",
          "message": "fix(ci): the CLA gate refused to run for the comment that signs it",
          "timestamp": "2026-09-01T23:18:11Z",
          "url": "https://github.com/nervosys/HyperMachine/pull/82/commits/f7731559805167da2bd7c5d4ef2ee0da9afeaa9c"
        },
        "date": 1788371253471,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 499.842,
            "range": "+/- 4.438",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2966.667,
            "range": "+/- 10.081",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 367.866,
            "range": "+/- 1.518",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 949.301,
            "range": "+/- 3.213",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 349.543,
            "range": "+/- 2.476",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 20804.006,
            "range": "+/- 99.38",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 586.858,
            "range": "+/- 3.759",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3465.03,
            "range": "+/- 21.862",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 456.83,
            "range": "+/- 1.653",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1106.407,
            "range": "+/- 12.174",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 427.111,
            "range": "+/- 2.064",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 10872.268,
            "range": "+/- 67.605",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2510.535,
            "range": "+/- 8.878",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 105.786,
            "range": "+/- 0.298",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 115.648,
            "range": "+/- 1.041",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1156.686,
            "range": "+/- 4.712",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1846.512,
            "range": "+/- 8.717",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 659.803,
            "range": "+/- 2.774",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 823.33,
            "range": "+/- 1.736",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1098.712,
            "range": "+/- 4.136",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12990.876,
            "range": "+/- 27.765",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 503.262,
            "range": "+/- 1.589",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3465.539,
            "range": "+/- 5.998",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 399.891,
            "range": "+/- 19.187",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 73610.002,
            "range": "+/- 371.088",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11259.878,
            "range": "+/- 37.236",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 373.916,
            "range": "+/- 1.673",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 16.656,
            "range": "+/- 0.045",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 95.725,
            "range": "+/- 0.433",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 20.278,
            "range": "+/- 0.085",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1391.299,
            "range": "+/- 6.337",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 31.654,
            "range": "+/- 0.061",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1915.021,
            "range": "+/- 6.359",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 14510.411,
            "range": "+/- 72.155",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 901.624,
            "range": "+/- 4.355",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 812701.69,
            "range": "+/- 1766.655",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 13317.291,
            "range": "+/- 135.488",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 283.15,
            "range": "+/- 0.699",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3475.281,
            "range": "+/- 26.906",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 134.256,
            "range": "+/- 0.422",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 51225.246,
            "range": "+/- 241.49",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2177.759,
            "range": "+/- 23.695",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1959032.731,
            "range": "+/- 21746.682",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 31681.557,
            "range": "+/- 449.256",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 736.826,
            "range": "+/- 4.451",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8142.908,
            "range": "+/- 108.185",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 270.036,
            "range": "+/- 0.961",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 129069.13,
            "range": "+/- 1979.455",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7745.772,
            "range": "+/- 168.121",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7397.638,
            "range": "+/- 20.517",
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
          "id": "c1c58dd62b145687e6442fe6b1eb03b22ff0bdf3",
          "message": "fix(ci): the CLA gate refused to run for the comment that signs it (#82)\n\nThe job's condition admitted two events: any `pull_request_target`, and an\n`issue_comment` whose body is exactly `recheck`. The signing phrase was\nnot in the list, so the one comment that records a signature was the one\ncomment the job would not run for.\n\nThe failure is silent from the pull request's side. The check simply stays\nred, while the run appears in the Actions list as `issue_comment /\nskipped` and `signatures/cla.json` stays `{\"signedContributors\": []}`.\nSigning again does not help, because the second attempt is skipped for the\nsame reason as the first.\n\nThe action's own README guards on both strings. This restores the second.\n\nThat makes three separate faults in this workflow, each of which alone was\nenough to make signing impossible: an action reference that did not\nresolve (fixed in #75), a signatures branch the token could not write\n(fixed in #75), and this. The check has never once recorded a signature.\n\n\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-02T10:26:32-07:00",
          "tree_id": "89363f48cdb344db4e7c2b962cbfd7935c27c974",
          "url": "https://github.com/nervosys/HyperMachine/commit/c1c58dd62b145687e6442fe6b1eb03b22ff0bdf3"
        },
        "date": 1788372528696,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 527.792,
            "range": "+/- 0.859",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3523.342,
            "range": "+/- 16.938",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 404.373,
            "range": "+/- 0.888",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1013.337,
            "range": "+/- 3.225",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 373.043,
            "range": "+/- 1.064",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 12494.556,
            "range": "+/- 61.658",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 635.684,
            "range": "+/- 1.5",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3858.544,
            "range": "+/- 13.112",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 479.02,
            "range": "+/- 0.264",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1186.569,
            "range": "+/- 4.569",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 445.586,
            "range": "+/- 1.142",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13455.533,
            "range": "+/- 24.799",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2684.609,
            "range": "+/- 7.392",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 99.906,
            "range": "+/- 0.255",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 130.632,
            "range": "+/- 5.89",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1210.016,
            "range": "+/- 4.476",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1962.107,
            "range": "+/- 9.248",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 640.014,
            "range": "+/- 1.131",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 823.372,
            "range": "+/- 1.541",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1062.179,
            "range": "+/- 1.357",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12012.877,
            "range": "+/- 6.882",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 497.172,
            "range": "+/- 0.519",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3244.128,
            "range": "+/- 1.217",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 358.682,
            "range": "+/- 0.948",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 81614.872,
            "range": "+/- 816.925",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11519.594,
            "range": "+/- 93.059",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 407.031,
            "range": "+/- 0.944",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 17.7,
            "range": "+/- 0.169",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 104.086,
            "range": "+/- 0.417",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 23.834,
            "range": "+/- 0.224",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1601.152,
            "range": "+/- 2.521",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 38.164,
            "range": "+/- 0.369",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2004.402,
            "range": "+/- 7.922",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15424.976,
            "range": "+/- 55.074",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 838.49,
            "range": "+/- 0.579",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 746871.856,
            "range": "+/- 297.991",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 11815.675,
            "range": "+/- 13.907",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 291.483,
            "range": "+/- 0.38",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3045.561,
            "range": "+/- 6.398",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 148.859,
            "range": "+/- 0.301",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46825.308,
            "range": "+/- 23.103",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2555.452,
            "range": "+/- 16.035",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2229793.043,
            "range": "+/- 5428.468",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35118.931,
            "range": "+/- 83.006",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 901.148,
            "range": "+/- 5.423",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9165.989,
            "range": "+/- 56.104",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 327.883,
            "range": "+/- 1.027",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 139211.401,
            "range": "+/- 224.213",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 6831.951,
            "range": "+/- 50.253",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7575.794,
            "range": "+/- 33.54",
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
          "id": "375a9345cf7be4852b78efa9f1d374b522c6a419",
          "message": "fix(ci): the baseline comparison failed builds on measurement noise (#76)\n\n* fix(ci): the baseline comparison failed builds on measurement noise\n\nFixing the Benchmarks job in #74 made this one visible for the first\ntime: benchmark-comparison declares `needs: benchmark`, and that job had\nnever completed a run, so this had never executed either.\n\nIt greps criterion's output for the word \"regressed\" and exited 1 on a\nmatch, with no threshold. Criterion prints that line for any benchmark\nmeasured slower than its baseline, however slightly, and the two runs\nbeing compared happen on one shared CI runner, minutes apart, with a full\nrebuild in between. A one percent wobble failed the build exactly as a\ntwo hundred percent regression would.\n\nThe evidence that this is noise rather than signal: it failed\nidentically, with a dozen \"Performance has regressed\" lines, on three\nDependabot pull requests bumping a Terraform provider, a Docker base\nimage and tock-registers. None of those can affect hv2-core's crypto\nbenchmarks.\n\nAlso worth noting: every other step in that job already carries\ncontinue-on-error, so the job tolerated the benchmarks themselves failing\nand then failed on noise in their output.\n\nReal regression alerting already exists and has both a threshold and a\nbaseline drawn from stored history -- github-action-benchmark at\nalert-threshold 150%, in the job above. This step's value is putting the\nnumbers in front of a reviewer, which it still does; it just no longer\nfails a build for having measured something on a busy machine.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\n\n* fix(ci): pull requests were writing the benchmark baseline they compare to\n\n`auto-push` was unconditional, so every event that ran this workflow\ncommitted its measurements to `gh-pages`. Of the first 20 stored\nmeasurements, 18 came from pull-request branches and only 2 from master\n-- and several of those branches have since been deleted, so the\nbaseline is largely measurements of work that was never merged.\n\nThat is a better explanation for the comparison failures than runner\nnoise alone: a master run was not comparing itself against the previous\nmaster run, it was comparing itself against whichever pull request\nhappened to benchmark last.\n\nHistory now comes from pushes only. A pull request still runs the\nbenchmarks and still receives its comparison comment; it just no longer\nrecords itself as the thing the next run measures against.\n\nThe stored history is left as it is. Pruning it means rewriting a data\nbranch, which is the repository owner's call, and the entries are\nharmless once nothing new is appended from a pull request.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv\n\n* fix(ci): correct a claim I made about this repository publishing nothing\n\nThe comment added in #74 said GitHub Pages was not enabled here, so\nnothing committed to `gh-pages` is published. That is wrong, and I wrote\nit. Pages is enabled with `source.branch` set to `gh-pages`:\n\n  {\"status\":\"errored\",\"html_url\":\"https://nervosys.github.io/HyperMachine/\",\n   \"build_type\":\"legacy\",\"source\":{\"branch\":\"gh-pages\",\"path\":\"/\"},\n   \"public\":true}\n\nThe 404 that led me to the wrong conclusion comes from the Pages builds\nfailing, not from Pages being off. The two most recent builds both report\n\"Page build failed\".\n\nThis matters beyond the comment. Everything the benchmark action commits\nis served publicly at nervosys.github.io/HyperMachine/dev/bench/, so the\n18 pull-request measurements described in the previous commit are on a\npublic page rather than in a private data file.\n\nCo-Authored-By: Claude Opus 5 <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv\n\n---------\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-02T12:14:29-07:00",
          "tree_id": "4243a38c142200cdbe5d96bdcd31707d373ebd2a",
          "url": "https://github.com/nervosys/HyperMachine/commit/375a9345cf7be4852b78efa9f1d374b522c6a419"
        },
        "date": 1788377605668,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 531.149,
            "range": "+/- 1.447",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 3360.658,
            "range": "+/- 14.661",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 508.358,
            "range": "+/- 2.018",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 1016.89,
            "range": "+/- 4.82",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 369.028,
            "range": "+/- 0.43",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 13585.393,
            "range": "+/- 52.685",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 614.701,
            "range": "+/- 1.31",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 3761.219,
            "range": "+/- 9.237",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 479.526,
            "range": "+/- 1.172",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 1183.922,
            "range": "+/- 3.651",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 447.639,
            "range": "+/- 1.334",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 13413.219,
            "range": "+/- 27.816",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2669.012,
            "range": "+/- 6.199",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 101.177,
            "range": "+/- 0.583",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 121.14,
            "range": "+/- 1.711",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 1191.741,
            "range": "+/- 1.594",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1906.118,
            "range": "+/- 2.263",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 663.826,
            "range": "+/- 1.873",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 843.863,
            "range": "+/- 2.785",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 1054.433,
            "range": "+/- 1.007",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 12088.013,
            "range": "+/- 37.274",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 495.842,
            "range": "+/- 0.688",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 3238.734,
            "range": "+/- 1.271",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 357.683,
            "range": "+/- 0.688",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 75661.468,
            "range": "+/- 397.951",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 11326.868,
            "range": "+/- 114.244",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 402.721,
            "range": "+/- 1.185",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 16.695,
            "range": "+/- 0.057",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 104.422,
            "range": "+/- 0.275",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 21.984,
            "range": "+/- 0.037",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1611.912,
            "range": "+/- 6.301",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 34.54,
            "range": "+/- 0.102",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 2164.033,
            "range": "+/- 31.445",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 15897.524,
            "range": "+/- 156.215",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 847.56,
            "range": "+/- 2.812",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 747126.844,
            "range": "+/- 264.616",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 12243.73,
            "range": "+/- 142.528",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 282.587,
            "range": "+/- 0.991",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 3037.284,
            "range": "+/- 2.75",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 141.365,
            "range": "+/- 0.863",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 46819.424,
            "range": "+/- 29.108",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2537.281,
            "range": "+/- 6.833",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 2248741.87,
            "range": "+/- 8063.661",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 35324.211,
            "range": "+/- 123.501",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 983.746,
            "range": "+/- 20.413",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 9127.479,
            "range": "+/- 52.591",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 328.622,
            "range": "+/- 1.43",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 140667.6,
            "range": "+/- 676.697",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 7230.012,
            "range": "+/- 90.719",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 7568.876,
            "range": "+/- 37.232",
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
          "id": "edff5253a3f5fdc8ddbeedec7c21cf6148dc7847",
          "message": "fix(agent): VM identifiers could collide, and one VM would replace another (#83)\n\n`uuid_v4` was neither a UUID nor unique:\n\n    let timestamp = SystemTime::now()...as_nanos();\n    format!(\"{:032x}\", timestamp)\n\nTwo calls landing in the same clock tick return the same string, and\n`LocalVmHost::create` finishes with `self.vms.write().insert(vm_id, ..)`.\nThe second VM therefore replaces the first, silently: no error, and\n`vm_count` reports one where two were created.\n\nmacOS has coarser `SystemTime` resolution than Windows or Linux, which is\nwhy CI caught this on the macOS lane only, in `list_reports_every_vm`:\n\n    assertion `left == right` failed\n      left: [\"b\"]\n     right: [\"a\", \"b\"]\n\nIdentifiers now come from the OS CSPRNG, so they are distinct and\nunpredictable. Ownership is still enforced by session and capability\nchecks rather than by an id being hard to guess -- this stops one agent's\nVM from taking another's place by accident, nothing more.\n\nThe function is renamed `fresh_id` because it never produced a UUID, and\nthe duplicate copy in communication.rs is removed rather than fixed\ntwice.\n\nOn the test: creating VMs in a loop and waiting for a duplicate only\nfails where the clock happens to be coarse, so it proves nothing on the\nplatforms where it passes -- exactly the trap the original test fell\ninto. The new test asserts the part that does not depend on timing. A\nnanosecond count since the epoch is about 2^61, and `{:032x}` pads it to\n32 digits, so every clock-derived id begins with sixteen zeros while\nrandom ones share no prefix. Verified by putting the old generator back:\nthe new test fails, and passes again once the fix returns.\n\n\nClaude-Session: https://claude.ai/code/session_01RsopUtvfyNzkKbbte56vZv\n\nCo-authored-by: Claude Opus 5 <noreply@anthropic.com>",
          "timestamp": "2026-09-02T15:46:54-07:00",
          "tree_id": "ec5082f19884ef7a6226ea233cf7aa17b8c61536",
          "url": "https://github.com/nervosys/HyperMachine/commit/edff5253a3f5fdc8ddbeedec7c21cf6148dc7847"
        },
        "date": 1788390200331,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "aes_gcm_decrypt/1024",
            "value": 409.216,
            "range": "+/- 0.906",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/16384",
            "value": 2639.825,
            "range": "+/- 6.161",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/256",
            "value": 391.34,
            "range": "+/- 0.419",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/4096",
            "value": 780.15,
            "range": "+/- 2.384",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/64",
            "value": 284.109,
            "range": "+/- 0.519",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_decrypt/65536",
            "value": 9532.343,
            "range": "+/- 36.418",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/1024",
            "value": 485.422,
            "range": "+/- 1.596",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/16384",
            "value": 2996.478,
            "range": "+/- 13.686",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/256",
            "value": 366.076,
            "range": "+/- 0.797",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/4096",
            "value": 909.977,
            "range": "+/- 2.52",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/64",
            "value": 336.408,
            "range": "+/- 0.954",
            "unit": "ns"
          },
          {
            "name": "aes_gcm_encrypt/65536",
            "value": 10407.399,
            "range": "+/- 30.521",
            "unit": "ns"
          },
          {
            "name": "fips_self_tests",
            "value": 2054.461,
            "range": "+/- 5.025",
            "unit": "ns"
          },
          {
            "name": "generate_aes128_key",
            "value": 77.882,
            "range": "+/- 0.327",
            "unit": "ns"
          },
          {
            "name": "generate_aes256_key",
            "value": 94.929,
            "range": "+/- 0.362",
            "unit": "ns"
          },
          {
            "name": "hkdf/128",
            "value": 922.932,
            "range": "+/- 1.185",
            "unit": "ns"
          },
          {
            "name": "hkdf/256",
            "value": 1494.711,
            "range": "+/- 1.228",
            "unit": "ns"
          },
          {
            "name": "hkdf/32",
            "value": 509.901,
            "range": "+/- 2.138",
            "unit": "ns"
          },
          {
            "name": "hkdf/64",
            "value": 644.143,
            "range": "+/- 0.748",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/1024",
            "value": 816.526,
            "range": "+/- 1.194",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/16384",
            "value": 9286.236,
            "range": "+/- 3.809",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/256",
            "value": 383.922,
            "range": "+/- 0.542",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/4096",
            "value": 2509.212,
            "range": "+/- 0.74",
            "unit": "ns"
          },
          {
            "name": "hmac_sha256/64",
            "value": 276.589,
            "range": "+/- 0.385",
            "unit": "ns"
          },
          {
            "name": "ontology/deserialize_ontology",
            "value": 61224.561,
            "range": "+/- 242.487",
            "unit": "ns"
          },
          {
            "name": "ontology/serialize_ontology",
            "value": 8686.892,
            "range": "+/- 64.37",
            "unit": "ns"
          },
          {
            "name": "random_bytes/1024",
            "value": 307.009,
            "range": "+/- 0.692",
            "unit": "ns"
          },
          {
            "name": "random_bytes/16",
            "value": 13.129,
            "range": "+/- 0.027",
            "unit": "ns"
          },
          {
            "name": "random_bytes/256",
            "value": 79.398,
            "range": "+/- 0.111",
            "unit": "ns"
          },
          {
            "name": "random_bytes/32",
            "value": 17.222,
            "range": "+/- 0.056",
            "unit": "ns"
          },
          {
            "name": "random_bytes/4096",
            "value": 1223.004,
            "range": "+/- 2.489",
            "unit": "ns"
          },
          {
            "name": "random_bytes/64",
            "value": 26.506,
            "range": "+/- 0.044",
            "unit": "ns"
          },
          {
            "name": "request_parsing/parse_create_vm_request",
            "value": 1625.097,
            "range": "+/- 6.692",
            "unit": "ns"
          },
          {
            "name": "request_parsing/serialize_list_response",
            "value": 12477.025,
            "range": "+/- 131.265",
            "unit": "ns"
          },
          {
            "name": "sha256/1024",
            "value": 656.692,
            "range": "+/- 1.198",
            "unit": "ns"
          },
          {
            "name": "sha256/1048576",
            "value": 579097.014,
            "range": "+/- 250.314",
            "unit": "ns"
          },
          {
            "name": "sha256/16384",
            "value": 9173.912,
            "range": "+/- 14.7",
            "unit": "ns"
          },
          {
            "name": "sha256/256",
            "value": 218.519,
            "range": "+/- 0.334",
            "unit": "ns"
          },
          {
            "name": "sha256/4096",
            "value": 2386.21,
            "range": "+/- 7.368",
            "unit": "ns"
          },
          {
            "name": "sha256/64",
            "value": 108.532,
            "range": "+/- 0.199",
            "unit": "ns"
          },
          {
            "name": "sha256/65536",
            "value": 36237.652,
            "range": "+/- 9.812",
            "unit": "ns"
          },
          {
            "name": "sha512/1024",
            "value": 2121.863,
            "range": "+/- 25.334",
            "unit": "ns"
          },
          {
            "name": "sha512/1048576",
            "value": 1774060.778,
            "range": "+/- 18887.329",
            "unit": "ns"
          },
          {
            "name": "sha512/16384",
            "value": 27951.898,
            "range": "+/- 329.818",
            "unit": "ns"
          },
          {
            "name": "sha512/256",
            "value": 747.753,
            "range": "+/- 8.975",
            "unit": "ns"
          },
          {
            "name": "sha512/4096",
            "value": 8109.431,
            "range": "+/- 120.831",
            "unit": "ns"
          },
          {
            "name": "sha512/64",
            "value": 256.997,
            "range": "+/- 1.362",
            "unit": "ns"
          },
          {
            "name": "sha512/65536",
            "value": 107771.416,
            "range": "+/- 144.066",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_anthropic_tools",
            "value": 5305.616,
            "range": "+/- 28.849",
            "unit": "ns"
          },
          {
            "name": "tool_formats/serialize_openai_tools",
            "value": 6028.36,
            "range": "+/- 88.263",
            "unit": "ns"
          }
        ]
      }
    ]
  }
}