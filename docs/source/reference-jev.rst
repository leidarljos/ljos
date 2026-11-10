:description: The keys of jev.toml and what each one changes.
:cover: https://leidarljos.github.io/ljos/_static/og.png

============
Jev settings
============


``~/.config/ljos/jev.toml``. The file is absent or ``enabled = false``: Jev is off.

=================== ======================================== =============================================================================================================================================================================================================
Key                 Default                                  Meaning
=================== ======================================== =============================================================================================================================================================================================================
``enabled``         ``false``                                turn Jev on; the call sends the prompt and claims off the machine
``backend``         ``jev``                                  which judge answers: ``jev``, ``chat`` (one JSON-mode chat completion at ``endpoint`` plus ``/chat/completions``, model ``model``) or ``command`` (the ``command`` argv, request on stdin, answers on stdout)
``command``         unset                                    the argv of the ``command`` backend; it prints ``{"answers": ...}`` inside ``budget_ms``
``key_file``        unset                                    file holding the key, one line, mode 0600
``key_cmd``         unset                                    command that prints the key on its first line, as an argv list; asked once per login
``model``           ``jev-1.13.0``                           model name; ``typesafe/jev-1.13`` on OpenRouter
``budget_ms``       2000                                     how long the hook waits for the answer
``endpoint``        ``https://api.typesafe.ai/v1/systemone`` API endpoint; ``https://openrouter.ai/api/alpha/decisions`` for OpenRouter; the base URL (``.../v1``) for ``chat``
``monthly_usd``     4.0                                      the month's spend in US dollars past which the hook stops asking
``min_words``       4                                        a prompt with fewer words is skipped
``min_candidates``  2                                        fewer candidate claims than this is skipped
``usd_per_mtok_in`` 0.042 on ``jev``, required on ``chat``   US dollars per million input tokens, for an API whose answer carries no cost. A ``chat`` judge with no price is not asked
``bears_at``        0.5                                      probability at which a claim bears on the prompt
``cue_at``          0.5                                      probability at which the prompt counts as a correction or a choice
``escalate_below``  0.8                                      a persona ballot under this confidence goes to a subagent
``cache_days``      7                                        days an identical request is answered from the cache; 0 turns it off
``key_env``         unset                                    environment variable holding the key
``[judges.NAME]``   none                                     a further judge: ``backend``, ``command``, ``key_file``, ``key_cmd``, ``key_env``, ``model``, ``endpoint``, ``budget_ms``, ``usd_per_mtok_in``, and ``weight`` (1.0) in a pool
``[route]``         every decision to ``default``            which judges answer each decision: ``prompt``, ``ballot``, ``audit``, ``review``, each a list of judge names; ``default`` is the judge the top-level keys describe
=================== ======================================== =============================================================================================================================================================================================================

====================================== ========================================================================================================================================
Path                                   Contents
====================================== ========================================================================================================================================
``$XDG_STATE_HOME/ljos/jev-cost.toml`` the month's calls and spend
``$XDG_STATE_HOME/ljos/jev-log.jsonl`` every Jev answer: claim ids, a prompt hash, latency, timeout or failure, backend and model. ``ljos judge-score`` joins it with outcomes
``$XDG_CACHE_HOME/ljos/jev``           the answer cache: a hash of the request and the reply, directory mode 0700, file mode 0600. The prompt and the claim text are not stored
``$XDG_RUNTIME_DIR/ljos/jev-key``      the default judge's key from ``key_cmd``, mode 0600; ``jev-key-NAME`` for a named judge
====================================== ========================================================================================================================================

The four routes are ``prompt``, ``ballot``, ``audit`` and ``review``. ``backend = jev`` posts to ``endpoint`` (TypeSafe's API, or OpenRouter at ``https://openrouter.ai/api/alpha/decisions`` with model ``typesafe/jev-1.13``). ``backend = chat`` posts one JSON-mode completion to ``endpoint`` plus ``/chat/completions`` and must set ``usd_per_mtok_in``. With ``backend = command``, the request stays on the machine. TypeSafe's terms say they will not train on customer data. They keep a perpetual licence to derive telemetry from inputs. Zero data retention needs an enterprise agreement and is off by default. This seat does not train on Jev's answers. A judge ballot is a trust row with ``to`` ``judge:MODEL``, and ``learn`` updates it.
