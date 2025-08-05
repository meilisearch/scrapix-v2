import eslint from '@eslint/js'
import tseslint from '@typescript-eslint/eslint-plugin'
import tseslintParser from '@typescript-eslint/parser'
import prettier from 'eslint-plugin-prettier'
import react from 'eslint-plugin-react'
import reactHooks from 'eslint-plugin-react-hooks'

const config = [
  {
    ignores: [
      'node_modules/**',
      'dist/**',
      '.next/**',
      'coverage/**',
      '*.js',
      '*.cjs',
      '*.mjs',
      'postcss.config.js',
      'next.config.js',
      'tailwind.config.ts'
    ]
  },
  eslint.configs.recommended,
  {
    files: ['**/*.ts', '**/*.tsx'],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: 'module',
      parser: tseslintParser,
      parserOptions: {
        project: ['./tsconfig.json'],
        tsconfigRootDir: import.meta.dirname,
        ecmaFeatures: {
          jsx: true
        }
      },
      globals: {
        process: 'readonly',
        console: 'readonly',
        __dirname: 'readonly',
        __filename: 'readonly',
        Buffer: 'readonly',
        setTimeout: 'readonly',
        clearTimeout: 'readonly',
        setInterval: 'readonly',
        clearInterval: 'readonly',
        NodeJS: 'readonly',
        React: 'readonly',
        JSX: 'readonly'
      }
    },
    plugins: {
      '@typescript-eslint': tseslint,
      'prettier': prettier,
      'react': react,
      'react-hooks': reactHooks
    },
    settings: {
      react: {
        version: 'detect'
      }
    },
    rules: {
      // Prettier configuration - enforce single quotes and no semicolons
      'prettier/prettier': [
        'error',
        {
          singleQuote: true,
          semi: false,
          arrowParens: 'always',
          bracketSpacing: true,
          trailingComma: 'es5',
          printWidth: 100,
          tabWidth: 2,
          useTabs: false,
          jsxSingleQuote: true
        }
      ],
      
      // TypeScript specific rules
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/explicit-function-return-type': 'off',
      '@typescript-eslint/no-unsafe-assignment': 'off',
      '@typescript-eslint/no-unsafe-member-access': 'off',
      '@typescript-eslint/no-unsafe-argument': 'off',
      '@typescript-eslint/no-misused-promises': 'off',
      '@typescript-eslint/ban-ts-comment': 'off',
      
      // General rules
      'no-unused-vars': 'off',
      '@typescript-eslint/no-unused-vars': ['warn', { 
        'argsIgnorePattern': '^_',
        'varsIgnorePattern': '^_',
        'caughtErrorsIgnorePattern': '^_'
      }],
      
      // Enforce consistent coding style
      'quotes': 'off',
      '@typescript-eslint/quotes': ['error', 'single', { 'avoidEscape': true }],
      'semi': 'off',
      '@typescript-eslint/semi': ['error', 'never'],
      'jsx-quotes': ['error', 'prefer-single'],
      
      // React specific rules
      'react/prop-types': 'off',
      'react/react-in-jsx-scope': 'off', // Not needed in Next.js
      'react/jsx-uses-react': 'off',
      'react/jsx-uses-vars': 'error',
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',
      
      // Next.js specific
      'no-console': ['warn', { allow: ['warn', 'error'] }],
      'prefer-const': 'error',
      'no-var': 'error',
      'object-shorthand': 'error',
      'prefer-arrow-callback': 'error'
    }
  }
]

export default config