#!/bin/bash
# this should analyze output from evaluator and propose a plan of actions to take
GOAL_FILE="./.newton/goal.md"
EVALUATOR_STATUS_FILE="./.newton/state/evaluator_status.md"
ADVISOR_RECOMMENDATIONS_FILE="./.newton/state/advisor_recommendations.md"
PROMPT="Your goals are set on file $GOAL_FILE. Read content of file $EVALUATOR_STATUS_FILE and propose a concise plan of list of actions. Don't write any data, only answer with the proposal."
# read file content to variable and pass to opencode run
#content = $(cat "$EVALUATOR_STATUS_FILE")
echo "$PROMPT"
opencode run "$PROMPT" | tee "$ADVISOR_RECOMMENDATIONS_FILE"
