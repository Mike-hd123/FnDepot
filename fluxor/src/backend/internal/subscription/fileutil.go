package subscription

import (
	"os"
)

// copyFile 复制文件
func copyFile(src, dst string) error {
	srcData, err := os.ReadFile(src)
	if err != nil {
		return err
	}
	return os.WriteFile(dst, srcData, 0644)
}
