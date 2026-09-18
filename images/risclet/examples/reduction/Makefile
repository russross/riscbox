.SUFFIXES:
.SUFFIXES: .o .s .out .xml .input .transcript

ALLSRC=$(wildcard *.s)
START=$(filter start.s, $(ALLSRC))
AOUTSRC=$(START) $(filter-out $(START), $(ALLSRC))
RISCLET_VERSION=v0.4.8

STEPPER_CMD=risclet run --check-abi $(AOUTSRC)
STEPPER_DIR=inputs
STEPPER_TIMEOUT=5
STEPPER_POSTERRORLINES=15
STEPPER_GRADE=false
export STEPPER_CMD STEPPER_DIR STEPPER_TIMEOUT STEPPER_POSTERRORLINES STEPPER_GRADE

all:	step

grade:	export STEPPER_GRADE := true
grade:
	@rm -f test_details.xml
	python3 lib/stepper

run:
	risclet run $(AOUTSRC)

step:
	python3 lib/stepper

trace:
	risclet trace $(AOUTSRC)

debug:
	risclet debug $(AOUTSRC)

clean:
	rm -f core *.o *.out *.xml
