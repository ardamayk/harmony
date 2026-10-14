package managedimport

import (
	"encoding/json"
	"fmt"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
)

type ValidationIssues []library.InspectionIssue

// encode stores issues as the JSON array kept in validation_issues columns.
func (issues ValidationIssues) encode() (string, error) {
	if issues == nil {
		return "[]", nil
	}
	data, err := json.Marshal(issues)
	if err != nil {
		return "", fmt.Errorf("encode import validation issues: %w", err)
	}
	return string(data), nil
}

func decodeValidationIssues(stored string) (ValidationIssues, error) {
	var issues ValidationIssues
	if err := json.Unmarshal([]byte(stored), &issues); err != nil {
		return nil, fmt.Errorf("decode import validation issues: %w", err)
	}
	return issues, nil
}

func (validationErr *ValidationError) validationIssues() ValidationIssues {
	if len(validationErr.Issues) > 0 {
		return validationErr.Issues
	}
	return ValidationIssues{{Code: library.InspectionErrorCode(validationErr.Code), Field: validationErr.Field, Reason: strictValidationReason(validationErr)}}
}
