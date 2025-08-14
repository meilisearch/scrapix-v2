FROM node:18-alpine

# Install basic dependencies
RUN apk add --no-cache \
    ca-certificates

WORKDIR /app

# Copy package files
COPY package.json ./
COPY yarn.lock ./

# Install dependencies
RUN yarn install --frozen-lockfile

# Copy source code
COPY . .

# Build the application
RUN yarn build

# Expose ports
EXPOSE 3000 8080

# Start the application
CMD ["yarn", "start"]