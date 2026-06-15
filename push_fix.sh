#!/bin/bash
TOKEN=$(gh auth token)
git remote set-url origin https://x-access-token:${TOKEN}@github.com/Gong-Mi/termux-app.git
git push origin work/googleplay-base
