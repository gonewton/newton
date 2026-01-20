# we want to capture output to a file but also show output on the terminal
GOAL_FILE="./.newton/goal.md"
ADVISOR_RECOMMENDATIONS_FILE="./.newton/advisor_recommendations.md"
PROMPT="Your goals are set on file $GOAL_FILE. Read content of file $ADVISOR_RECOMMENDATIONS_FILE which contains important recomendations on how to make progress"
echo "$PROMPT"
opencode run "$PROMPT"