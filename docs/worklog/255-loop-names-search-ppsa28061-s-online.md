# 2026-09-01 (/loop) - names search: PPSA28061's online import is un-nameable locally


Ran `orbistoun-cli names` on PPSA28061 (1604s): 3.8B generated + 3018 published + 4611 module-string
candidates, 0 of 377 unnamed imports matched - including `libSceNpCppWebApi::0xa9721c01ca796f63`, the
online leaderboard call that aborts the JSON init. So that wall needs an external symbol source for
libSceNpCppWebApi, not more generator effort - don't re-run the 27-minute search. (The tool's own note:
"no published standard name matched ... a strong sign --suffix-hex is wrong" for these modules.)
Confirms PPSA28061 is online-blocked; the loop pivots to PPSA02664/25872's image+0x7b5890 next.

