package sample

import "testing"

func TestPasses(t *testing.T) {}

func TestFails(t *testing.T) {
	t.Fatalf("intentional failure for tapcue integration fixture")
}
