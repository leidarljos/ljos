`ljos sync --adopt HOST` on a machine whose host name is not already a log file name, such as one with a space, now commits this machine's updated log with the deletion of the gone host's log.
The export wrote `bad host.jsonl.age` while the adopt commit staged `bad-host.jsonl.age`, so peers retired the gone host's copies and never received the adopted atoms.
The host name is now folded once, and export, adopt and every log path use that name; a log left under the raw name is not imported back as another machine's.
