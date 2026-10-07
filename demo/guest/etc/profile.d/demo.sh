# Start each login in the shared project with the standard Alpine shell prompt.
cd /shared
export HISTFILE=/home/riscbox/.ash_history
export PS1='\h:\w\$ '
