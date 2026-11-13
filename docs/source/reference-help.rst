:description: What the ljos help command prints, taken from this version.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=========
Help text
=========


``ljos --help`` groups commands under Start, Daily, Tracker, Memory,
Coordination and Admin.
The first group is what a new seat needs.
A command a runner or git calls, and other rarely typed commands, stay
off that list.
They still run.
``ljos help --all`` lists every command.

.. BEGIN ljos-sample help

.. code:: text

   $ ljos --version
   ljos 0.41.11
   $ ljos --help
   Memory, a work tracker and a command guard for coding runners

   Usage: ljos <COMMAND>

   Start:
     doctor       Check that ljos is ready, and print the rows that need action

   Options:
     -h, --help     Print help
     -V, --version  Print version

   A seat is the name in `LJOS_SEAT`: the person or agent this process acts as. Start with `ljos doctor`, `ljos prefer "..."`, `ljos policy -- COMMAND` and `ljos onboard`. `ljos protocol` explains the rest. `ljos help --all` lists every command.
   $ ljos doctor --help
   Check that ljos is ready, and print the rows that need action.

   Exit 1 when a required program, the memory store or its token does not answer, the tool server a runner starts or the cloud signing key does not, the note queue is damaged, or the tracker directory is not writable. A tracker that has not been created yet does not fail this command.

   Example: `ljos doctor`

   Usage: ljos doctor [OPTIONS]

   Options:
         --all
             Print every row, the ones that answer included
             
             [alias: --verbose]

         --fix
             Refresh runner skills and runner env files this ljos wrote, and replace a stale cloud guard rule file. A skill edited by hand, an env file ljos did not write, and a cloud guard file that is missing or not a known older version are left in place

     -h, --help
             Print help (see a summary with '-h')

.. END ljos-sample help
