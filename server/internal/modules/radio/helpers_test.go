package radio

import "database/sql"

func newTestService(sqlDB *sql.DB) *Service {
	return NewService(NewRepository(sqlDB))
}
