# Start each login in the shared project, with shell history in writable tmpfs.
cd /workspace
export HISTFILE=/home/demo/.ash_history
export PS1='demo@riscbox:\w\$ '
