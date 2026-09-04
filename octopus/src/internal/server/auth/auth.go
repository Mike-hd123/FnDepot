package auth

import (
	"crypto/rand"
	"math/big"
	"time"

	"github.com/bestruirui/octopus/internal/conf"
	"github.com/bestruirui/octopus/internal/op"
	"github.com/golang-jwt/jwt/v5"
)

func GenerateJWTToken(expiresSec int) (string, int, error) {
	now := time.Now()
	maxAge := int((15 * time.Minute).Seconds())
	if expiresSec > 0 {
		maxAge = expiresSec
	} else if expiresSec == -1 {
		maxAge = int((30 * 24 * time.Hour).Seconds())
	}
	claims := &jwt.RegisteredClaims{
		IssuedAt:  jwt.NewNumericDate(now),
		NotBefore: jwt.NewNumericDate(now),
		Issuer:    conf.APP_NAME,
		ExpiresAt: jwt.NewNumericDate(now.Add(time.Duration(maxAge) * time.Second)),
	}
	user := op.UserGet()
	secret := user.Username + user.Password
	token, err := jwt.NewWithClaims(jwt.SigningMethodHS256, claims).SignedString([]byte(secret))
	if err != nil {
		return "", 0, err
	}
	return token, maxAge, nil
}

func VerifyJWTToken(token string) bool {
	jwtToken, err := jwt.Parse(token, func(token *jwt.Token) (interface{}, error) {
		user := op.UserGet()
		secret := user.Username + user.Password
		return []byte(secret), nil
	})
	if err != nil || !jwtToken.Valid {
		return false
	}
	return true
}

func GenerateAPIKey() string {
	const keyChars = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
	b := make([]byte, 48)
	maxI := big.NewInt(int64(len(keyChars)))
	for i := range b {
		n, err := rand.Int(rand.Reader, maxI)
		if err != nil {
			return ""
		}
		b[i] = keyChars[n.Int64()]
	}
	return "sk-" + conf.APP_NAME + "-" + string(b)
}
